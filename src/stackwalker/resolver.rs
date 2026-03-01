//! 堆栈解析器实现
//!
//! 将原始堆栈地址转换为带符号信息的完整堆栈帧列表。
//! 使用 SymbolManager 批量解析地址，支持缓存优化。

use crate::error::{Result, SymbolError};
use crate::symbols::SymbolManager;
use crate::types::{Address, CallStack, ModuleInfo, ProcessId, StackFrame, SymbolInfo};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::{debug, trace, warn};

/// 堆栈解析器
///
/// 负责将堆栈地址列表解析为包含符号信息的调用堆栈。
/// 使用批处理和缓存策略优化解析性能。
///
/// # 线程安全
///
/// 所有方法都是线程安全的，内部使用 Mutex 保护共享状态。
pub struct StackResolver {
    /// 符号管理器
    symbol_manager: Arc<SymbolManager>,
    /// 解析缓存（进程 ID -> (地址 -> 堆栈帧)）
    cache: Mutex<HashMap<ProcessId, HashMap<Address, CachedFrame>>>,
    /// 最大缓存条目数
    max_cache_entries: Mutex<usize>,
    /// 解析统计
    stats: Mutex<ResolveStats>,
}

/// 缓存的堆栈帧
#[derive(Debug, Clone)]
struct CachedFrame {
    frame: StackFrame,
    cached_at: Instant,
    access_count: u64,
}

/// 解析统计信息
#[derive(Debug, Clone)]
pub struct ResolveStats {
    /// 总解析请求数
    pub total_requests: u64,
    /// 缓存命中数
    pub cache_hits: u64,
    /// 成功解析数
    pub resolved: u64,
    /// 解析失败数（显示为 ???）
    pub failed: u64,
    /// 批量解析次数
    pub batch_resolves: u64,
}

impl StackResolver {
    /// 创建新的堆栈解析器
    ///
    /// # 参数
    /// - `symbol_manager`: 符号管理器实例
    ///
    /// # 示例
    ///
    /// ```rust,no_run
    /// use std::sync::Arc;
    /// use profiler::stackwalker::StackResolver;
    /// use profiler::symbols::SymbolManager;
    ///
    /// let symbol_manager = Arc::new(SymbolManager::new());
    /// let resolver = StackResolver::new(symbol_manager);
    /// ```
    pub fn new(symbol_manager: Arc<SymbolManager>) -> Self {
        debug!("Creating StackResolver");

        Self {
            symbol_manager,
            cache: Mutex::new(HashMap::new()),
            max_cache_entries: Mutex::new(10000),
            stats: Mutex::new(ResolveStats::default()),
        }
    }

    /// 创建带缓存限制的解析器
    ///
    /// # 参数
    /// - `symbol_manager`: 符号管理器实例
    /// - `max_cache_entries`: 每个进程的最大缓存条目数
    pub fn with_cache_limit(symbol_manager: Arc<SymbolManager>, max_cache_entries: usize) -> Self {
        debug!("Creating StackResolver with cache limit: {}", max_cache_entries);

        Self {
            symbol_manager,
            cache: Mutex::new(HashMap::new()),
            max_cache_entries: Mutex::new(max_cache_entries),
            stats: Mutex::new(ResolveStats::default()),
        }
    }

    /// 解析整个堆栈
    ///
    /// 将地址列表批量解析为带符号信息的调用堆栈。
    /// 使用缓存减少重复解析的 API 调用。
    ///
    /// # 参数
    /// - `addresses`: 堆栈地址列表（从栈顶到栈底）
    /// - `process_id`: 进程 ID
    ///
    /// # 返回
    /// 解析后的调用堆栈
    pub fn resolve_stack(&self, addresses: &[Address], process_id: ProcessId) -> Result<CallStack> {
        trace!("Resolving stack with {} addresses for process {}", addresses.len(), process_id);

        let mut call_stack = CallStack::with_capacity(addresses.len());
        let mut uncached_addresses = Vec::new();
        let mut address_to_position = HashMap::new();

        // 获取最大缓存条目数
        let max_entries = self.get_max_cache_entries();

        // 更新统计
        {
            let mut stats = self.stats.lock().map_err(|_| {
                SymbolError::new("Failed to lock stats")
            })?;
            stats.total_requests += addresses.len() as u64;
        }

        // 首先检查缓存
        {
            let cache = self.cache.lock().map_err(|_| {
                SymbolError::new("Failed to lock cache")
            })?;

            if let Some(process_cache) = cache.get(&process_id) {
                for (pos, &address) in addresses.iter().enumerate() {
                    if let Some(cached) = process_cache.get(&address) {
                        // 缓存命中
                        call_stack.push(cached.frame.clone());
                        let mut stats = self.stats.lock().map_err(|_| {
                            SymbolError::new("Failed to lock stats")
                        })?;
                        stats.cache_hits += 1;
                    } else {
                        // 缓存未命中，加入批量解析列表
                        uncached_addresses.push(address);
                        address_to_position.insert(address, pos);
                    }
                }
            } else {
                // 该进程没有缓存，全部需要解析
                uncached_addresses.extend(addresses);
                for (pos, &address) in addresses.iter().enumerate() {
                    address_to_position.insert(address, pos);
                }
            }
        }

        // 批量解析未缓存的地址
        if !uncached_addresses.is_empty() {
            trace!("Batch resolving {} uncached addresses", uncached_addresses.len());

            let frames = self.batch_resolve(&uncached_addresses, process_id)?;

            // 将解析结果放入缓存并构建调用堆栈
            {
                let mut cache = self.cache.lock().map_err(|_| {
                    SymbolError::new("Failed to lock cache")
                })?;

                let process_cache = cache.entry(process_id).or_insert_with(HashMap::new);

                for (address, frame) in uncached_addresses.iter().zip(frames.iter()) {
                    // 缓存控制：如果缓存太大，清除旧的条目
                    if process_cache.len() >= max_entries {
                        process_cache.clear(); // 简化处理：直接清空
                    }

                    process_cache.insert(*address, CachedFrame {
                        frame: frame.clone(),
                        cached_at: Instant::now(),
                        access_count: 1,
                    });
                }
            }

            // 合并结果到调用堆栈
            // 注意：需要保持原始顺序
            let mut all_frames: Vec<(usize, StackFrame)> = Vec::with_capacity(addresses.len());

            // 添加缓存命中的帧
            for (pos, &address) in addresses.iter().enumerate() {
                if !uncached_addresses.contains(&address) {
                    // 这是缓存命中的帧
                    let cache = self.cache.lock().map_err(|_| {
                        SymbolError::new("Failed to lock cache")
                    })?;
                    if let Some(process_cache) = cache.get(&process_id) {
                        if let Some(cached) = process_cache.get(&address) {
                            all_frames.push((pos, cached.frame.clone()));
                        }
                    }
                }
            }

            // 添加新解析的帧
            for (address, frame) in uncached_addresses.iter().zip(frames.iter()) {
                if let Some(&pos) = address_to_position.get(address) {
                    all_frames.push((pos, frame.clone()));
                }
            }

            // 按位置排序并构建调用堆栈
            all_frames.sort_by_key(|(pos, _)| *pos);
            for (_, frame) in all_frames {
                call_stack.push(frame);
            }
        }

        // 更新统计
        {
            let mut stats = self.stats.lock().map_err(|_| {
                SymbolError::new("Failed to lock stats")
            })?;
            stats.batch_resolves += 1;
        }

        trace!("Resolved stack with {} frames", call_stack.len());
        Ok(call_stack)
    }

    /// 解析单个堆栈帧
    ///
    /// # 参数
    /// - `address`: 指令地址
    /// - `process_id`: 进程 ID
    ///
    /// # 返回
    /// 解析后的堆栈帧
    pub fn resolve_frame(&self, address: Address, process_id: ProcessId) -> Result<StackFrame> {
        trace!("Resolving frame at 0x{:016X} for process {}", address, process_id);

        // 首先检查缓存
        {
            let cache = self.cache.lock().map_err(|_| {
                SymbolError::new("Failed to lock cache")
            })?;

            if let Some(process_cache) = cache.get(&process_id) {
                if let Some(cached) = process_cache.get(&address) {
                    trace!("Cache hit for address 0x{:016X}", address);

                    // 更新统计
                    let mut stats = self.stats.lock().map_err(|_| {
                        SymbolError::new("Failed to lock stats")
                    })?;
                    stats.cache_hits += 1;

                    return Ok(cached.frame.clone());
                }
            }
        }

        // 使用符号管理器解析
        let frame = self.resolve_single_address(address, process_id)?;

        // 缓存结果
        {
            let mut cache = self.cache.lock().map_err(|_| {
                SymbolError::new("Failed to lock cache")
            })?;

            let process_cache = cache.entry(process_id).or_insert_with(HashMap::new);
            process_cache.insert(address, CachedFrame {
                frame: frame.clone(),
                cached_at: Instant::now(),
                access_count: 1,
            });
        }

        Ok(frame)
    }

    /// 批量解析地址
    fn batch_resolve(&self, addresses: &[Address], process_id: ProcessId) -> Result<Vec<StackFrame>> {
        let mut frames = Vec::with_capacity(addresses.len());
        let mut resolved_count = 0u64;
        let mut failed_count = 0u64;

        // 确保符号管理器有该进程的解析器
        let _ = self.symbol_manager.create_resolver(process_id);

        for &address in addresses {
            match self.resolve_single_address(address, process_id) {
                Ok(frame) => {
                    if frame.has_symbol_info() {
                        resolved_count += 1;
                    } else {
                        failed_count += 1;
                    }
                    frames.push(frame);
                }
                Err(e) => {
                    warn!("Failed to resolve address 0x{:016X}: {}", address, e);
                    failed_count += 1;

                    // 创建失败标记帧
                    frames.push(StackFrame::new(address));
                }
            }
        }

        // 更新统计
        {
            let mut stats = self.stats.lock().map_err(|_| {
                SymbolError::new("Failed to lock stats")
            })?;
            stats.resolved += resolved_count;
            stats.failed += failed_count;
        }

        Ok(frames)
    }

    /// 解析单个地址
    fn resolve_single_address(&self, address: Address, process_id: ProcessId) -> Result<StackFrame> {
        // 尝试使用符号管理器解析
        match self.symbol_manager.resolve_sample(process_id, address) {
            Ok(Some(symbol)) => {
                let frame = StackFrame {
                    address,
                    module_name: symbol.module.clone(),
                    function_name: Some(symbol.name.clone()),
                    file_name: symbol.source_file.clone(),
                    line_number: symbol.line_number,
                    column_number: None,
                    offset: None,
                };
                Ok(frame)
            }
            Ok(None) => {
                // 无法解析，尝试获取模块信息
                let module_name = self.try_get_module_name(address, process_id);

                let frame = if let Some(name) = module_name {
                    // 显示为 "模块名+偏移"
                    StackFrame {
                        address,
                        module_name: Some(name),
                        function_name: None,
                        file_name: None,
                        line_number: None,
                        column_number: None,
                        offset: None,
                    }
                } else {
                    // 完全无法解析，显示为 ???
                    StackFrame::new(address)
                };

                Ok(frame)
            }
            Err(e) => {
                // 解析出错，返回基本帧
                trace!("Symbol resolution failed for 0x{:016X}: {}", address, e);
                Ok(StackFrame::new(address))
            }
        }
    }

    /// 尝试获取地址所在的模块名
    fn try_get_module_name(&self, address: Address, process_id: ProcessId) -> Option<String> {
        if let Some(resolver) = self.symbol_manager.get_resolver(process_id) {
            if let Ok(resolver) = resolver.lock() {
                if let Ok(module) = resolver.get_module_at_address(address) {
                    return Some(module.name);
                }
            }
        }
        None
    }

    /// 获取解析统计信息
    pub fn get_stats(&self) -> ResolveStats {
        self.stats.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// 重置统计信息
    pub fn reset_stats(&self) {
        if let Ok(mut stats) = self.stats.lock() {
            *stats = ResolveStats::default();
            debug!("Resolve statistics reset");
        }
    }

    /// 清除指定进程的缓存
    pub fn clear_cache(&self, process_id: ProcessId) {
        if let Ok(mut cache) = self.cache.lock() {
            if cache.remove(&process_id).is_some() {
                debug!("Cleared cache for process {}", process_id);
            }
        }
    }

    /// 清除所有缓存
    pub fn clear_all_cache(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
            debug!("Cleared all cache");
        }
    }

    /// 设置最大缓存条目数
    pub fn set_max_cache_entries(&self, limit: usize) {
        if let Ok(mut max) = self.max_cache_entries.lock() {
            *max = limit;
            debug!("Max cache entries set to {}", limit);
        }
    }

    /// 获取最大缓存条目数
    fn get_max_cache_entries(&self) -> usize {
        self.max_cache_entries.lock().map(|m| *m).unwrap_or(10000)
    }

    /// 获取当前缓存大小
    pub fn get_cache_size(&self) -> usize {
        self.cache.lock().map(|c| {
            c.values().map(|v| v.len()).sum()
        }).unwrap_or(0)
    }

    /// 获取指定进程的缓存大小
    pub fn get_process_cache_size(&self, process_id: ProcessId) -> usize {
        self.cache.lock().map(|c| {
            c.get(&process_id).map(|v| v.len()).unwrap_or(0)
        }).unwrap_or(0)
    }
}

impl Default for ResolveStats {
    fn default() -> Self {
        Self {
            total_requests: 0,
            cache_hits: 0,
            resolved: 0,
            failed: 0,
            batch_resolves: 0,
        }
    }
}

/// 创建未知符号的堆栈帧
pub fn create_unknown_frame(address: Address) -> StackFrame {
    StackFrame {
        address,
        module_name: None,
        function_name: Some("???".to_string()),
        file_name: None,
        line_number: None,
        column_number: None,
        offset: None,
    }
}

/// 创建带模块名+偏移的堆栈帧
pub fn create_module_offset_frame(address: Address, module_name: &str, module_base: Address) -> StackFrame {
    let offset = address.saturating_sub(module_base);
    StackFrame {
        address,
        module_name: Some(module_name.to_string()),
        function_name: Some(format!("+0x{:X}", offset)),
        file_name: None,
        line_number: None,
        column_number: None,
        offset: Some(offset),
    }
}

/// 堆栈帧格式化选项
#[derive(Debug, Clone, Copy)]
pub struct FrameFormatOptions {
    /// 显示模块名
    pub show_module: bool,
    /// 显示源文件信息
    pub show_source: bool,
    /// 显示地址
    pub show_address: bool,
    /// 显示偏移
    pub show_offset: bool,
}

impl Default for FrameFormatOptions {
    fn default() -> Self {
        Self {
            show_module: true,
            show_source: false,
            show_address: true,
            show_offset: false,
        }
    }
}

/// 格式化堆栈帧为字符串
pub fn format_frame(frame: &StackFrame, options: FrameFormatOptions) -> String {
    let mut parts = Vec::new();

    if options.show_address {
        parts.push(format!("0x{:016X}", frame.address));
    }

    let symbol_part = if let Some(func) = &frame.function_name {
        if let Some(module) = &frame.module_name {
            if options.show_module {
                format!("{}!{}", module, func)
            } else {
                func.clone()
            }
        } else {
            func.clone()
        }
    } else if let Some(module) = &frame.module_name {
        if options.show_offset && frame.offset.is_some() {
            format!("{}+0x{:X}", module, frame.offset.unwrap())
        } else if options.show_module {
            module.clone()
        } else {
            "???".to_string()
        }
    } else {
        "???".to_string()
    };
    parts.push(symbol_part);

    if options.show_source {
        if let (Some(file), Some(line)) = (&frame.file_name, frame.line_number) {
            parts.push(format!("[{}:{}]", file, line));
        }
    }

    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_unknown_frame() {
        let frame = create_unknown_frame(0x00401000);
        assert_eq!(frame.address, 0x00401000);
        assert_eq!(frame.function_name, Some("???".to_string()));
    }

    #[test]
    fn test_create_module_offset_frame() {
        let frame = create_module_offset_frame(0x00401050, "test.dll", 0x00400000);
        assert_eq!(frame.address, 0x00401050);
        assert_eq!(frame.module_name, Some("test.dll".to_string()));
        assert_eq!(frame.offset, Some(0x1050));
    }

    #[test]
    fn test_format_frame() {
        let frame = StackFrame::with_symbol(0x00401000, "test.dll", "TestFunction");

        let formatted = format_frame(&frame, FrameFormatOptions::default());
        assert!(formatted.contains("test.dll!TestFunction"));
        assert!(formatted.contains("0x00401000"));
    }

    #[test]
    fn test_resolve_stats_default() {
        let stats = ResolveStats::default();
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.cache_hits, 0);
        assert_eq!(stats.resolved, 0);
    }
}
