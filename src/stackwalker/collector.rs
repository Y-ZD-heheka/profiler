//! 堆栈收集器实现
//!
//! 整合堆栈展开和解析，从 ETW 采样事件中收集完整的调用堆栈信息。
//! 提供回调接口将收集到的堆栈传递给分析模块。

use crate::error::{ProfilerError, Result};
use crate::etw::SampledProfileEvent;
use crate::stackwalker::{is_kernel_address, StackCallback, StackResolver, StackUnwinder, StackWalkContext};
use crate::types::{Address, CallStack, ModuleInfo, ProcessId, StackFrame, SymbolInfo, ThreadId, Timestamp};
use std::sync::Mutex;
use tracing::{debug, error, trace, warn};
use windows::Win32::System::Diagnostics::Debug::CONTEXT;

/// 收集的完整堆栈信息
///
/// 包含从采样事件或上下文收集的完整调用堆栈及其元数据。
#[derive(Debug, Clone, PartialEq)]
pub struct CollectedStack {
    /// 进程 ID
    pub process_id: ProcessId,
    /// 线程 ID
    pub thread_id: ThreadId,
    /// 事件时间戳
    pub timestamp: Timestamp,
    /// 解析后的堆栈帧列表
    pub frames: Vec<ResolvedFrame>,
    /// 用户态堆栈深度
    pub user_stack_depth: usize,
    /// 内核态堆栈深度
    pub kernel_stack_depth: usize,
    /// 是否为纯内核堆栈
    pub is_kernel_stack: bool,
}

/// 解析后的堆栈帧
///
/// 包含地址、符号和模块信息的完整堆栈帧。
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedFrame {
    /// 指令地址
    pub address: Address,
    /// 符号信息（如果可用）
    pub symbol: Option<SymbolInfo>,
    /// 模块信息（如果可用）
    pub module: Option<ModuleInfo>,
    /// 帧类型（用户态/内核态）
    pub frame_type: FrameType,
}

/// 帧类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    /// 用户态帧
    User,
    /// 内核态帧
    Kernel,
    /// 未知类型
    Unknown,
}

/// 堆栈收集器
///
/// 整合堆栈展开器和解析器，提供从各种数据源收集完整堆栈的功能。
///
/// # 使用示例
///
/// ```rust,no_run
/// use std::sync::Arc;
/// use profiler::stackwalker::{StackCollector, EtwStackUnwinder, StackResolver};
/// use profiler::symbols::SymbolManager;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let symbol_manager = Arc::new(SymbolManager::new());
/// let unwinder = Box::new(EtwStackUnwinder::new());
/// let resolver = StackResolver::new(symbol_manager);
///
/// let collector = StackCollector::new(unwinder, resolver);
/// # Ok(())
/// # }
/// ```
pub struct StackCollector {
    /// 堆栈展开器
    unwinder: Box<dyn StackUnwinder>,
    /// 堆栈解析器
    resolver: StackResolver,
    /// 回调列表
    callbacks: Mutex<Vec<Box<dyn StackCallback>>>,
    /// 收集统计
    stats: Mutex<CollectorStats>,
}

/// 收集器统计信息
#[derive(Debug, Clone, Default)]
pub struct CollectorStats {
    /// 收集的总堆栈数
    pub total_collected: u64,
    /// 失败的收集次数
    pub failed_collections: u64,
    /// 用户态帧数
    pub user_frames: u64,
    /// 内核态帧数
    pub kernel_frames: u64,
    /// 平均堆栈深度
    pub avg_stack_depth: f64,
}

impl CollectedStack {
    /// 创建新的收集堆栈
    pub fn new(
        process_id: ProcessId,
        thread_id: ThreadId,
        timestamp: Timestamp,
        frames: Vec<ResolvedFrame>,
    ) -> Self {
        let user_stack_depth = frames.iter().filter(|f| f.frame_type == FrameType::User).count();
        let kernel_stack_depth = frames.iter().filter(|f| f.frame_type == FrameType::Kernel).count();
        let is_kernel_stack = user_stack_depth == 0 && kernel_stack_depth > 0;

        Self {
            process_id,
            thread_id,
            timestamp,
            frames,
            user_stack_depth,
            kernel_stack_depth,
            is_kernel_stack,
        }
    }

    /// 获取堆栈总深度
    pub fn total_depth(&self) -> usize {
        self.frames.len()
    }

    /// 获取栈顶帧（当前执行位置）
    pub fn top_frame(&self) -> Option<&ResolvedFrame> {
        self.frames.first()
    }

    /// 获取栈底帧（调用链起点）
    pub fn bottom_frame(&self) -> Option<&ResolvedFrame> {
        self.frames.last()
    }

    /// 转换为火焰图格式字符串
    pub fn to_flame_graph_string(&self) -> String {
        self.frames
            .iter()
            .rev()
            .map(|f| f.display_name())
            .collect::<Vec<_>>()
            .join(";")
    }

    /// 转换为 CallStack 类型
    pub fn to_call_stack(&self) -> CallStack {
        let stack_frames: Vec<StackFrame> = self.frames.iter().map(|f| {
            StackFrame {
                address: f.address,
                module_name: f.module.as_ref().map(|m| m.name.clone()),
                function_name: f.symbol.as_ref().map(|s| s.name.clone()),
                file_name: f.symbol.as_ref().and_then(|s| s.source_file.clone()),
                line_number: f.symbol.as_ref().and_then(|s| s.line_number),
                column_number: None,
                offset: None,
            }
        }).collect();

        CallStack {
            frames: stack_frames,
            depth: self.frames.len(),
            is_complete: true,
        }
    }

    /// 检查是否包含内核态帧
    pub fn has_kernel_frames(&self) -> bool {
        self.kernel_stack_depth > 0
    }

    /// 检查是否包含用户态帧
    pub fn has_user_frames(&self) -> bool {
        self.user_stack_depth > 0
    }
}

impl ResolvedFrame {
    /// 创建新的解析帧
    pub fn new(address: Address) -> Self {
        Self {
            address,
            symbol: None,
            module: None,
            frame_type: FrameType::Unknown,
        }
    }

    /// 创建带类型的解析帧
    pub fn with_type(address: Address, frame_type: FrameType) -> Self {
        Self {
            address,
            symbol: None,
            module: None,
            frame_type,
        }
    }

    /// 设置符号信息
    pub fn with_symbol(mut self, symbol: SymbolInfo) -> Self {
        self.symbol = Some(symbol);
        self
    }

    /// 设置模块信息
    pub fn with_module(mut self, module: ModuleInfo) -> Self {
        self.module = Some(module);
        self
    }

    /// 设置帧类型
    pub fn with_frame_type(mut self, frame_type: FrameType) -> Self {
        self.frame_type = frame_type;
        self
    }

    /// 获取显示名称
    pub fn display_name(&self) -> String {
        if let Some(symbol) = &self.symbol {
            if let Some(module) = &self.module {
                format!("{}!{}", module.name, symbol.name)
            } else if let Some(module_name) = &symbol.module {
                format!("{}!{}", module_name, symbol.name)
            } else {
                symbol.name.clone()
            }
        } else if let Some(module) = &self.module {
            let offset = self.address.saturating_sub(module.base_address);
            format!("{}+0x{:X}", module.name, offset)
        } else {
            format!("0x{:016X}", self.address)
        }
    }

    /// 获取简短显示名称
    pub fn short_name(&self) -> String {
        self.symbol.as_ref()
            .map(|s| s.name.clone())
            .or_else(|| self.module.as_ref().map(|m| m.name.clone()))
            .unwrap_or_else(|| format!("0x{:X}", self.address))
    }

    /// 检查是否有符号信息
    pub fn has_symbol(&self) -> bool {
        self.symbol.is_some()
    }

    /// 检查是否有模块信息
    pub fn has_module(&self) -> bool {
        self.module.is_some()
    }
}

impl StackCollector {
    /// 创建新的堆栈收集器
    ///
    /// # 参数
    /// - `unwinder`: 堆栈展开器实例
    /// - `resolver`: 堆栈解析器实例
    pub fn new(unwinder: Box<dyn StackUnwinder>, resolver: StackResolver) -> Self {
        debug!("Creating StackCollector");

        Self {
            unwinder,
            resolver,
            callbacks: Mutex::new(Vec::new()),
            stats: Mutex::new(CollectorStats::default()),
        }
    }

    /// 从采样事件收集完整堆栈
    ///
    /// # 参数
    /// - `sample`: ETW 采样配置文件事件
    ///
    /// # 返回
    /// 收集到的完整堆栈信息
    pub fn collect_from_sample(&self, sample: &SampledProfileEvent) -> Result<CollectedStack> {
        trace!(
            "Collecting stack from sample: pid={}, tid={}, ip=0x{:016X}",
            sample.process_id,
            sample.thread_id,
            sample.instruction_pointer
        );

        // 1. 展开堆栈获取地址列表
        let addresses = self.unwinder.unwind_from_event(sample)?;

        if addresses.is_empty() {
            warn!("No addresses unwound from sample");
            return Err(ProfilerError::Generic(
                "Stack unwinding returned no addresses".to_string()
            ));
        }

        // 2. 解析地址为堆栈帧
        let call_stack = self.resolver.resolve_stack(&addresses, sample.process_id)?;

        // 3. 构建解析后的帧列表
        let mut resolved_frames = Vec::with_capacity(call_stack.frames.len());
        for (i, frame) in call_stack.frames.iter().enumerate() {
            let address = addresses.get(i).copied().unwrap_or(frame.address);
            let frame_type = if is_kernel_address(address) {
                FrameType::Kernel
            } else {
                FrameType::User
            };

            let resolved = ResolvedFrame {
                address,
                symbol: frame.function_name.as_ref().map(|name| SymbolInfo {
                    address,
                    name: name.clone(),
                    module: frame.module_name.clone(),
                    source_file: frame.file_name.clone(),
                    line_number: frame.line_number,
                }),
                module: frame.module_name.as_ref().map(|name| ModuleInfo {
                    base_address: 0, // 未知基地址
                    size: 0,
                    name: name.clone(),
                    path: None,
                    pdb_path: None,
                    timestamp: 0,
                    checksum: 0,
                }),
                frame_type,
            };
            resolved_frames.push(resolved);
        }

        // 4. 创建收集的堆栈
        let collected = CollectedStack::new(
            sample.process_id,
            sample.thread_id,
            sample.timestamp,
            resolved_frames,
        );

        // 5. 更新统计
        self.update_stats(&collected);

        // 6. 触发回调
        self.notify_callbacks(&collected);

        trace!("Collected stack with {} frames", collected.total_depth());
        Ok(collected)
    }

    /// 从 CPU 上下文收集堆栈
    ///
    /// # 参数
    /// - `context`: CPU 上下文结构
    /// - `process_id`: 进程 ID
    /// - `thread_id`: 线程 ID
    /// - `timestamp`: 时间戳
    ///
    /// # 返回
    /// 收集到的完整堆栈信息
    pub fn collect_with_context(
        &self,
        context: &CONTEXT,
        process_id: ProcessId,
        thread_id: ThreadId,
        timestamp: Timestamp,
    ) -> Result<CollectedStack> {
        trace!(
            "Collecting stack from context: pid={}, tid={}",
            process_id,
            thread_id
        );

        // 1. 展开堆栈
        let addresses = self.unwinder.unwind_from_context(context, process_id)?;

        if addresses.is_empty() {
            warn!("No addresses unwound from context");
            return Err(ProfilerError::Generic(
                "Stack unwinding from context returned no addresses".to_string()
            ));
        }

        // 2. 解析地址
        let call_stack = self.resolver.resolve_stack(&addresses, process_id)?;

        // 3. 构建解析后的帧列表
        let mut resolved_frames = Vec::with_capacity(call_stack.frames.len());
        for (i, frame) in call_stack.frames.iter().enumerate() {
            let address = addresses.get(i).copied().unwrap_or(frame.address);
            let frame_type = if is_kernel_address(address) {
                FrameType::Kernel
            } else {
                FrameType::User
            };

            let resolved = ResolvedFrame {
                address,
                symbol: frame.function_name.as_ref().map(|name| SymbolInfo {
                    address,
                    name: name.clone(),
                    module: frame.module_name.clone(),
                    source_file: frame.file_name.clone(),
                    line_number: frame.line_number,
                }),
                module: frame.module_name.as_ref().map(|name| ModuleInfo {
                    base_address: 0,
                    size: 0,
                    name: name.clone(),
                    path: None,
                    pdb_path: None,
                    timestamp: 0,
                    checksum: 0,
                }),
                frame_type,
            };
            resolved_frames.push(resolved);
        }

        // 4. 创建收集的堆栈
        let collected = CollectedStack::new(
            process_id,
            thread_id,
            timestamp,
            resolved_frames,
        );

        // 5. 更新统计
        self.update_stats(&collected);

        // 6. 触发回调
        self.notify_callbacks(&collected);

        Ok(collected)
    }

    /// 添加回调
    pub fn add_callback(&self, callback: Box<dyn StackCallback>) {
        if let Ok(mut callbacks) = self.callbacks.lock() {
            callbacks.push(callback);
            debug!("Added stack callback, total: {}", callbacks.len());
        }
    }

    /// 移除所有回调
    pub fn clear_callbacks(&self) {
        if let Ok(mut callbacks) = self.callbacks.lock() {
            callbacks.clear();
            debug!("Cleared all callbacks");
        }
    }

    /// 获取统计信息
    pub fn get_stats(&self) -> CollectorStats {
        self.stats.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// 重置统计信息
    pub fn reset_stats(&self) {
        if let Ok(mut stats) = self.stats.lock() {
            *stats = CollectorStats::default();
            debug!("Collector statistics reset");
        }
    }

    /// 获取展开器引用
    pub fn unwinder(&self) -> &dyn StackUnwinder {
        self.unwinder.as_ref()
    }

    /// 获取解析器引用
    pub fn resolver(&self) -> &StackResolver {
        &self.resolver
    }

    /// 更新统计
    fn update_stats(&self, stack: &CollectedStack) {
        if let Ok(mut stats) = self.stats.lock() {
            stats.total_collected += 1;
            stats.user_frames += stack.user_stack_depth as u64;
            stats.kernel_frames += stack.kernel_stack_depth as u64;

            // 更新平均深度
            let total_depth = stack.total_depth() as u64;
            let total_stacks = stats.total_collected;
            stats.avg_stack_depth = (stats.avg_stack_depth * (total_stacks - 1) as f64
                + total_depth as f64) / total_stacks as f64;
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

    /// 处理错误并通知
    fn handle_error(&self, error: &ProfilerError, context: &StackWalkContext) {
        if let Ok(mut stats) = self.stats.lock() {
            stats.failed_collections += 1;
        }

        if let Ok(mut callbacks) = self.callbacks.lock() {
            for callback in callbacks.iter_mut() {
                callback.on_stack_error(error, context);
            }
        }
    }
}

impl FrameType {
    /// 获取显示名称
    pub fn display_name(&self) -> &'static str {
        match self {
            FrameType::User => "USER",
            FrameType::Kernel => "KERNEL",
            FrameType::Unknown => "UNKNOWN",
        }
    }

    /// 检查是否为用户态
    pub fn is_user(&self) -> bool {
        matches!(self, FrameType::User)
    }

    /// 检查是否为内核态
    pub fn is_kernel(&self) -> bool {
        matches!(self, FrameType::Kernel)
    }
}

/// 堆栈收集构建器
///
/// 用于方便地构建和配置堆栈收集器。
pub struct StackCollectorBuilder {
    unwinder: Option<Box<dyn StackUnwinder>>,
    resolver: Option<StackResolver>,
}

impl StackCollectorBuilder {
    /// 创建新的构建器
    pub fn new() -> Self {
        Self {
            unwinder: None,
            resolver: None,
        }
    }

    /// 设置展开器
    pub fn with_unwinder(mut self, unwinder: Box<dyn StackUnwinder>) -> Self {
        self.unwinder = Some(unwinder);
        self
    }

    /// 设置解析器
    pub fn with_resolver(mut self, resolver: StackResolver) -> Self {
        self.resolver = Some(resolver);
        self
    }

    /// 构建收集器
    pub fn build(self) -> Result<StackCollector> {
        let unwinder = self.unwinder.ok_or_else(|| {
            ProfilerError::Generic("Stack unwinder is required".to_string())
        })?;

        let resolver = self.resolver.ok_or_else(|| {
            ProfilerError::Generic("Stack resolver is required".to_string())
        })?;

        Ok(StackCollector::new(unwinder, resolver))
    }
}

impl Default for StackCollectorBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collected_stack_creation() {
        let frames = vec![
            ResolvedFrame::with_type(0x00401000, FrameType::User),
            ResolvedFrame::with_type(0x00402000, FrameType::User),
        ];

        let stack = CollectedStack::new(1234, 5678, 1000, frames);
        assert_eq!(stack.process_id, 1234);
        assert_eq!(stack.thread_id, 5678);
        assert_eq!(stack.timestamp, 1000);
        assert_eq!(stack.total_depth(), 2);
        assert_eq!(stack.user_stack_depth, 2);
        assert!(!stack.is_kernel_stack);
    }

    #[test]
    fn test_resolved_frame_display() {
        let frame = ResolvedFrame::new(0x00401000);
        assert_eq!(frame.display_name(), "0x0000000000401000");

        let frame = ResolvedFrame::new(0x00401000)
            .with_module(ModuleInfo::new(0x00400000, 0x10000, "test.dll"));
        assert!(frame.display_name().contains("test.dll"));
    }

    #[test]
    fn test_frame_type() {
        assert!(FrameType::User.is_user());
        assert!(!FrameType::User.is_kernel());
        assert!(FrameType::Kernel.is_kernel());
        assert_eq!(FrameType::User.display_name(), "USER");
        assert_eq!(FrameType::Kernel.display_name(), "KERNEL");
    }

    #[test]
    fn test_collector_stats_default() {
        let stats = CollectorStats::default();
        assert_eq!(stats.total_collected, 0);
        assert_eq!(stats.failed_collections, 0);
        assert_eq!(stats.avg_stack_depth, 0.0);
    }
}
