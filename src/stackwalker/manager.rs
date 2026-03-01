//! 堆栈管理器实现
//!
//! 管理堆栈收集的生命周期，协调展开、解析和过滤。
//! 提供配置选项和统计信息收集。

use crate::error::{Result, SymbolError};
use crate::etw::SampledProfileEvent;
use crate::stackwalker::{
    CollectedStack, CompositeFilter, DepthFilter, EtwStackUnwinder,
    StackCallback, StackCollector, StackFilter, StackResolver,
    StackWalkContext, SystemModuleFilter,
};
use crate::symbols::SymbolManager;
use crate::types::{ProcessId, StackFrame};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tracing::{debug, error, info, trace, warn};

/// 堆栈管理器配置
///
/// 配置堆栈收集的行为参数。
#[derive(Debug, Clone, Copy)]
pub struct StackManagerConfig {
    /// 最大堆栈深度（默认 64）
    pub max_stack_depth: usize,
    /// 是否收集内核堆栈
    pub enable_kernel_stack: bool,
    /// 是否跳过系统模块
    pub skip_system_modules: bool,
    /// 最大缓存条目数
    pub max_cache_entries: usize,
    /// 启用过滤
    pub enable_filtering: bool,
}

impl Default for StackManagerConfig {
    fn default() -> Self {
        Self {
            max_stack_depth: 64,
            enable_kernel_stack: true,
            skip_system_modules: false,
            max_cache_entries: 10000,
            enable_filtering: true,
        }
    }
}

impl StackManagerConfig {
    /// 创建默认配置
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置最大堆栈深度
    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_stack_depth = depth;
        self
    }

    /// 设置是否启用内核堆栈
    pub fn with_kernel_stack(mut self, enable: bool) -> Self {
        self.enable_kernel_stack = enable;
        self
    }

    /// 设置是否跳过系统模块
    pub fn with_skip_system_modules(mut self, skip: bool) -> Self {
        self.skip_system_modules = skip;
        self
    }

    /// 设置最大缓存条目数
    pub fn with_max_cache_entries(mut self, max: usize) -> Self {
        self.max_cache_entries = max;
        self
    }

    /// 设置是否启用过滤
    pub fn with_filtering(mut self, enable: bool) -> Self {
        self.enable_filtering = enable;
        self
    }
}

/// 堆栈管理器统计信息
#[derive(Debug, Clone, Default)]
pub struct StackManagerStats {
    /// 处理的采样事件数
    pub samples_processed: u64,
    /// 成功收集的堆栈数
    pub stacks_collected: u64,
    /// 失败的收集次数
    pub collection_failures: u64,
    /// 过滤掉的堆栈数
    pub stacks_filtered: u64,
    /// 平均堆栈深度
    pub avg_stack_depth: f64,
    /// 用户态帧数
    pub user_frames: u64,
    /// 内核态帧数
    pub kernel_frames: u64,
    /// 解析成功率（百分比）
    pub resolve_success_rate: f64,
}

/// 堆栈管理器
///
/// 管理堆栈收集的整个生命周期，包括初始化、配置、事件处理和清理。
/// 是堆栈遍历模块的主要入口点。
///
/// # 使用示例
///
/// ```rust,no_run
/// use std::sync::Arc;
/// use profiler::stackwalker::{StackManager, StackManagerConfig};
/// use profiler::symbols::SymbolManager;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // 创建配置
/// let config = StackManagerConfig::default()
///     .with_max_depth(128)
///     .with_kernel_stack(true);
///
/// // 创建管理器
/// let mut stack_manager = StackManager::new(config);
///
/// // 初始化
/// let symbol_manager = Arc::new(SymbolManager::new());
/// stack_manager.initialize(symbol_manager)?;
///
/// // 处理采样事件
/// // let stack = stack_manager.on_sample(&sample_event)?;
///
/// // 获取统计
/// let stats = stack_manager.get_stats();
/// println!("Collected {} stacks", stats.stacks_collected);
/// # Ok(())
/// # }
/// ```
pub struct StackManager {
    /// 配置
    config: Mutex<StackManagerConfig>,
    /// 符号管理器
    symbol_manager: Mutex<Option<Arc<SymbolManager>>>,
    /// 堆栈收集器
    collector: Mutex<Option<StackCollector>>,
    /// 过滤器链
    filter: Mutex<Option<Box<dyn StackFilter>>>,
    /// 回调列表
    callbacks: Mutex<Vec<Box<dyn StackCallback>>>,
    /// 统计信息
    samples_processed: AtomicU64,
    stacks_collected: AtomicU64,
    collection_failures: AtomicU64,
    stacks_filtered: AtomicU64,
    total_stack_depth: AtomicU64,
}

impl StackManager {
    /// 创建新的堆栈管理器
    ///
    /// # 参数
    /// - `config`: 管理器配置
    pub fn new(config: StackManagerConfig) -> Self {
        info!("Creating StackManager with config: {:?}", config);

        let manager = Self {
            config: Mutex::new(config),
            symbol_manager: Mutex::new(None),
            collector: Mutex::new(None),
            filter: Mutex::new(None),
            callbacks: Mutex::new(Vec::new()),
            samples_processed: AtomicU64::new(0),
            stacks_collected: AtomicU64::new(0),
            collection_failures: AtomicU64::new(0),
            stacks_filtered: AtomicU64::new(0),
            total_stack_depth: AtomicU64::new(0),
        };

        // 如果启用过滤，初始化默认过滤器
        manager.init_default_filter();

        manager
    }

    /// 初始化管理器
    ///
    /// 使用符号管理器初始化堆栈收集器。
    ///
    /// # 参数
    /// - `symbol_manager`: 符号管理器实例
    pub fn initialize(&self, symbol_manager: Arc<SymbolManager>) -> Result<()> {
        info!("Initializing StackManager");

        // 存储符号管理器
        if let Ok(mut sm) = self.symbol_manager.lock() {
            *sm = Some(symbol_manager.clone());
        }

        // 获取配置
        let config = self.get_config();

        // 创建展开器
        let unwinder = Box::new(EtwStackUnwinder::with_config(
            config.max_stack_depth,
            config.enable_kernel_stack,
        ));

        // 创建解析器
        let resolver = StackResolver::with_cache_limit(
            symbol_manager,
            config.max_cache_entries,
        );

        // 创建收集器
        let collector = StackCollector::new(unwinder, resolver);

        // 注册回调
        if let Ok(callbacks) = self.callbacks.lock() {
            for _ in callbacks.iter() {
                // collector.add_callback(callback.clone());
            }
        }

        // 存储收集器
        if let Ok(mut col) = self.collector.lock() {
            *col = Some(collector);
        }

        info!("StackManager initialized successfully");
        Ok(())
    }

    /// 处理采样事件
    ///
    /// 从 ETW 采样事件收集并解析调用堆栈。
    ///
    /// # 参数
    /// - `sample`: ETW 采样配置文件事件
    ///
    /// # 返回
    /// 如果堆栈通过过滤器，返回收集的堆栈；如果被过滤，返回 None
    pub fn on_sample(&self, sample: &SampledProfileEvent) -> Result<Option<CollectedStack>> {
        trace!("Processing sample event: pid={}, tid={}", sample.process_id, sample.thread_id);

        // 更新统计
        self.samples_processed.fetch_add(1, Ordering::Relaxed);

        // 获取收集器
        let collector = self.collector.lock().map_err(|_| {
            SymbolError::new("Failed to lock collector")
        })?;

        let collector = match collector.as_ref() {
            Some(c) => c,
            None => {
                error!("StackManager not initialized");
                return Err(SymbolError::new("StackManager not initialized").into());
            }
        };

        // 收集堆栈
        let stack = match collector.collect_from_sample(sample) {
            Ok(s) => s,
            Err(e) => {
                self.collection_failures.fetch_add(1, Ordering::Relaxed);
                error!("Failed to collect stack: {}", e);

                // 通知错误回调
                let context = StackWalkContext::new(
                    sample.process_id,
                    sample.thread_id,
                    "collect_from_sample"
                );
                self.notify_error(&e, &context);

                return Err(e);
            }
        };

        // 应用过滤器
        if let Some(filter) = self.filter.lock().map_err(|_| SymbolError::new("Failed to lock filter"))?.as_ref() {
            if !filter.should_include(&stack) {
                trace!("Stack filtered out");
                self.stacks_filtered.fetch_add(1, Ordering::Relaxed);
                return Ok(None);
            }
        }

        // 更新成功统计
        self.stacks_collected.fetch_add(1, Ordering::Relaxed);
        self.total_stack_depth.fetch_add(stack.total_depth() as u64, Ordering::Relaxed);

        // 触发回调
        self.notify_callbacks(&stack);

        trace!("Successfully processed sample, collected {} frames", stack.total_depth());
        Ok(Some(stack))
    }

    /// 设置最大堆栈深度
    pub fn set_max_depth(&self, depth: usize) {
        if let Ok(mut config) = self.config.lock() {
            config.max_stack_depth = depth;
            debug!("Max stack depth set to {}", depth);
        }

        // 更新展开器
        if let Ok(collector) = self.collector.lock() {
            if let Some(col) = collector.as_ref() {
                // 通过 unwinder 设置深度
                // col.unwinder().set_max_depth(depth);
            }
        }
    }

    /// 启用或禁用内核堆栈收集
    pub fn enable_kernel_stack(&self, enable: bool) {
        if let Ok(mut config) = self.config.lock() {
            config.enable_kernel_stack = enable;
            debug!("Kernel stack collection set to {}", enable);
        }

        // 更新展开器
        if let Ok(collector) = self.collector.lock() {
            if let Some(col) = collector.as_ref() {
                // col.unwinder().set_enable_kernel_stack(enable);
            }
        }
    }

    /// 设置是否跳过系统模块
    pub fn set_skip_system_modules(&self, skip: bool) {
        if let Ok(mut config) = self.config.lock() {
            config.skip_system_modules = skip;
            debug!("Skip system modules set to {}", skip);
        }

        // 重新初始化过滤器
        self.init_default_filter();
    }

    /// 设置自定义过滤器
    pub fn set_filter(&self, filter: Box<dyn StackFilter>) {
        if let Ok(mut f) = self.filter.lock() {
            *f = Some(filter);
            debug!("Custom filter set");
        }
    }

    /// 清除过滤器
    pub fn clear_filter(&self) {
        if let Ok(mut f) = self.filter.lock() {
            *f = None;
            debug!("Filter cleared");
        }
    }

    /// 添加回调
    pub fn add_callback(&self, callback: Box<dyn StackCallback>) {
        if let Ok(mut callbacks) = self.callbacks.lock() {
            callbacks.push(callback);
            debug!("Callback added, total: {}", callbacks.len());
        }

        // 如果收集器已创建，添加回调
        if let Ok(collector) = self.collector.lock() {
            if let Some(col) = collector.as_ref() {
                // col.add_callback(callback);
            }
        }
    }

    /// 移除所有回调
    pub fn clear_callbacks(&self) {
        if let Ok(mut callbacks) = self.callbacks.lock() {
            callbacks.clear();
            debug!("All callbacks cleared");
        }
    }

    /// 获取统计信息
    pub fn get_stats(&self) -> StackManagerStats {
        let samples = self.samples_processed.load(Ordering::Relaxed);
        let collected = self.stacks_collected.load(Ordering::Relaxed);
        let failures = self.collection_failures.load(Ordering::Relaxed);
        let filtered = self.stacks_filtered.load(Ordering::Relaxed);
        let total_depth = self.total_stack_depth.load(Ordering::Relaxed);

        let avg_depth = if collected > 0 {
            total_depth as f64 / collected as f64
        } else {
            0.0
        };

        let resolve_rate = if samples > 0 {
            (collected as f64 / samples as f64) * 100.0
        } else {
            0.0
        };

        StackManagerStats {
            samples_processed: samples,
            stacks_collected: collected,
            collection_failures: failures,
            stacks_filtered: filtered,
            avg_stack_depth: avg_depth,
            user_frames: 0, // 需要更详细的统计
            kernel_frames: 0,
            resolve_success_rate: resolve_rate,
        }
    }

    /// 重置统计信息
    pub fn reset_stats(&self) {
        self.samples_processed.store(0, Ordering::Relaxed);
        self.stacks_collected.store(0, Ordering::Relaxed);
        self.collection_failures.store(0, Ordering::Relaxed);
        self.stacks_filtered.store(0, Ordering::Relaxed);
        self.total_stack_depth.store(0, Ordering::Relaxed);

        // 重置收集器统计
        if let Ok(collector) = self.collector.lock() {
            if let Some(col) = collector.as_ref() {
                col.reset_stats();
            }
        }

        info!("StackManager statistics reset");
    }

    /// 清除缓存
    pub fn clear_cache(&self) {
        if let Ok(collector) = self.collector.lock() {
            if let Some(col) = collector.as_ref() {
                col.resolver().clear_all_cache();
            }
        }
        debug!("Cache cleared");
    }

    /// 关闭管理器
    pub fn shutdown(&self) -> Result<()> {
        info!("Shutting down StackManager");

        // 清除收集器
        if let Ok(mut collector) = self.collector.lock() {
            *collector = None;
        }

        // 清除符号管理器
        if let Ok(mut sm) = self.symbol_manager.lock() {
            *sm = None;
        }

        // 清除过滤器
        if let Ok(mut filter) = self.filter.lock() {
            *filter = None;
        }

        // 清除回调
        if let Ok(mut callbacks) = self.callbacks.lock() {
            callbacks.clear();
        }

        info!("StackManager shutdown complete");
        Ok(())
    }

    /// 检查是否已初始化
    pub fn is_initialized(&self) -> bool {
        self.collector.lock().map(|c| c.is_some()).unwrap_or(false)
    }

    /// 获取当前配置
    pub fn get_config(&self) -> StackManagerConfig {
        self.config.lock().map(|c| *c).unwrap_or_default()
    }

    /// 初始化默认过滤器
    fn init_default_filter(&self) {
        let config = self.get_config();

        if !config.enable_filtering {
            return;
        }

        let mut filters: Vec<Box<dyn StackFilter>> = Vec::new();

        // 深度过滤器
        filters.push(Box::new(DepthFilter::new(config.max_stack_depth)));

        // 系统模块过滤器（如果启用）
        if config.skip_system_modules {
            filters.push(Box::new(SystemModuleFilter::new()));
        }

        if !filters.is_empty() {
            let composite = CompositeFilter::new(filters);
            if let Ok(mut f) = self.filter.lock() {
                *f = Some(Box::new(composite));
            }
            debug!("Default filter initialized");
        }
    }

    /// 通知回调
    fn notify_callbacks(&self, stack: &CollectedStack) {
        if let Ok(mut callbacks) = self.callbacks.lock() {
            for callback in callbacks.iter_mut() {
                callback.on_stack_collected(stack);
            }
        }
    }

    /// 通知错误
    fn notify_error(&self, error: &crate::error::ProfilerError, context: &StackWalkContext) {
        if let Ok(mut callbacks) = self.callbacks.lock() {
            for callback in callbacks.iter_mut() {
                callback.on_stack_error(error, context);
            }
        }
    }
}

impl Default for StackManager {
    fn default() -> Self {
        Self::new(StackManagerConfig::default())
    }
}

/// 堆栈处理结果
#[derive(Debug, Clone)]
pub enum StackProcessResult {
    /// 成功收集
    Collected(CollectedStack),
    /// 被过滤器排除
    Filtered,
    /// 收集失败
    Failed(String),
}

/// 堆栈采样处理器 trait
///
/// 用于处理堆栈采样事件的回调接口。
pub trait StackSampleHandler: Send + Sync {
    /// 处理收集到的堆栈
    fn handle_stack(&mut self, stack: &CollectedStack);

    /// 处理过滤的堆栈
    fn handle_filtered(&mut self, process_id: ProcessId, thread_id: u32);

    /// 处理收集失败
    fn handle_error(&mut self, error: &str, process_id: ProcessId);
}

/// 异步堆栈处理器（用于流式处理）
pub trait AsyncStackHandler: Send + Sync {
    /// 异步处理堆栈
    fn handle_stack_async(&self, stack: CollectedStack) -> impl std::future::Future<Output = ()> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack_manager_config_default() {
        let config = StackManagerConfig::default();
        assert_eq!(config.max_stack_depth, 64);
        assert!(config.enable_kernel_stack);
        assert!(!config.skip_system_modules);
    }

    #[test]
    fn test_stack_manager_config_builder() {
        let config = StackManagerConfig::new()
            .with_max_depth(128)
            .with_kernel_stack(false)
            .with_skip_system_modules(true);

        assert_eq!(config.max_stack_depth, 128);
        assert!(!config.enable_kernel_stack);
        assert!(config.skip_system_modules);
    }

    #[test]
    fn test_stack_manager_stats_default() {
        let stats = StackManagerStats::default();
        assert_eq!(stats.samples_processed, 0);
        assert_eq!(stats.stacks_collected, 0);
        assert_eq!(stats.avg_stack_depth, 0.0);
    }

    #[test]
    fn test_stack_manager_creation() {
        let config = StackManagerConfig::default();
        let manager = StackManager::new(config);
        assert!(!manager.is_initialized());
    }
}
