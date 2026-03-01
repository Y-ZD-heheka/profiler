//! 核心数据结构定义模块
//!
//! 定义性能分析器使用的基础类型、采样事件、堆栈帧、调用堆栈
//! 以及统计信息等核心数据结构。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// 基础类型别名
// ============================================================================

/// 进程 ID 类型别名
pub type ProcessId = u32;

/// 线程 ID 类型别名
pub type ThreadId = u32;

/// 内存地址类型别名
pub type Address = u64;

/// 时间戳类型别名（微秒级 Unix 时间戳）
pub type Timestamp = u64;

// ============================================================================
// 采样事件
// ============================================================================

/// 采样事件
///
/// 表示在特定时间点采集到的 CPU 采样事件，包含进程、线程和指令指针信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SampleEvent {
    /// 事件时间戳（微秒级）
    pub timestamp: Timestamp,
    /// 进程 ID
    pub process_id: ProcessId,
    /// 线程 ID
    pub thread_id: ThreadId,
    /// 指令指针（当前执行的代码地址）
    pub instruction_pointer: Address,
    /// 处理器核心 ID（如果可用）
    pub processor_core: Option<u16>,
    /// 优先级（如果可用）
    pub priority: Option<u8>,
}

impl SampleEvent {
    /// 创建新的采样事件
    ///
    /// # 参数
    /// - `timestamp`: 事件时间戳
    /// - `process_id`: 进程 ID
    /// - `thread_id`: 线程 ID
    /// - `instruction_pointer`: 指令指针地址
    pub fn new(
        timestamp: Timestamp,
        process_id: ProcessId,
        thread_id: ThreadId,
        instruction_pointer: Address,
    ) -> Self {
        Self {
            timestamp,
            process_id,
            thread_id,
            instruction_pointer,
            processor_core: None,
            priority: None,
        }
    }

    /// 设置处理器核心 ID
    pub fn with_processor_core(mut self, core: u16) -> Self {
        self.processor_core = Some(core);
        self
    }

    /// 设置线程优先级
    pub fn with_priority(mut self, priority: u8) -> Self {
        self.priority = Some(priority);
        self
    }

    /// 检查是否为指定进程的事件
    pub fn is_process(&self, pid: ProcessId) -> bool {
        self.process_id == pid
    }

    /// 检查是否为指定线程的事件
    pub fn is_thread(&self, tid: ThreadId) -> bool {
        self.thread_id == tid
    }
}

impl Default for SampleEvent {
    fn default() -> Self {
        Self {
            timestamp: 0,
            process_id: 0,
            thread_id: 0,
            instruction_pointer: 0,
            processor_core: None,
            priority: None,
        }
    }
}

// ============================================================================
// 堆栈帧
// ============================================================================

/// 堆栈帧
///
/// 表示调用堆栈中的一个栈帧，包含地址和符号信息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StackFrame {
    /// 指令地址
    pub address: Address,
    /// 模块名称（如 kernel32.dll）
    pub module_name: Option<String>,
    /// 函数名称（如 CreateFileW）
    pub function_name: Option<String>,
    /// 源文件名
    pub file_name: Option<String>,
    /// 行号
    pub line_number: Option<u32>,
    /// 列号（可选）
    pub column_number: Option<u32>,
    /// 偏移量（相对于函数起始地址）
    pub offset: Option<u64>,
}

impl StackFrame {
    /// 创建新的堆栈帧（仅包含地址）
    pub fn new(address: Address) -> Self {
        Self {
            address,
            module_name: None,
            function_name: None,
            file_name: None,
            line_number: None,
            column_number: None,
            offset: None,
        }
    }

    /// 创建带符号信息的堆栈帧
    pub fn with_symbol(
        address: Address,
        module: impl Into<String>,
        function: impl Into<String>,
    ) -> Self {
        Self {
            address,
            module_name: Some(module.into()),
            function_name: Some(function.into()),
            file_name: None,
            line_number: None,
            column_number: None,
            offset: None,
        }
    }

    /// 设置源文件信息
    pub fn with_source(
        mut self,
        file: impl Into<String>,
        line: u32,
        column: Option<u32>,
    ) -> Self {
        self.file_name = Some(file.into());
        self.line_number = Some(line);
        self.column_number = column;
        self
    }

    /// 设置模块名称
    pub fn with_module(mut self, module: impl Into<String>) -> Self {
        self.module_name = Some(module.into());
        self
    }

    /// 设置函数名称
    pub fn with_function(mut self, function: impl Into<String>) -> Self {
        self.function_name = Some(function.into());
        self
    }

    /// 获取完整的函数签名
    pub fn full_function_name(&self) -> String {
        match (&self.module_name, &self.function_name) {
            (Some(module), Some(func)) => format!("{}!{}", module, func),
            (None, Some(func)) => func.clone(),
            (Some(module), None) => format!("{}!0x{:016X}", module, self.address),
            (None, None) => format!("0x{:016X}", self.address),
        }
    }

    /// 获取简短显示字符串
    pub fn short_name(&self) -> String {
        self.function_name
            .clone()
            .unwrap_or_else(|| format!("0x{:016X}", self.address))
    }

    /// 检查是否有符号信息
    pub fn has_symbol_info(&self) -> bool {
        self.function_name.is_some() || self.module_name.is_some()
    }

    /// 检查是否有源文件信息
    pub fn has_source_info(&self) -> bool {
        self.file_name.is_some() && self.line_number.is_some()
    }
}

// ============================================================================
// 调用堆栈
// ============================================================================

/// 调用堆栈
///
/// 表示一个完整的调用堆栈，包含多个堆栈帧。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CallStack {
    /// 堆栈帧列表（从栈顶到栈底）
    pub frames: Vec<StackFrame>,
    /// 堆栈深度
    pub depth: usize,
    /// 是否完整（未被截断）
    pub is_complete: bool,
}

impl CallStack {
    /// 创建空的调用堆栈
    pub fn new() -> Self {
        Self {
            frames: Vec::new(),
            depth: 0,
            is_complete: true,
        }
    }

    /// 创建具有指定容量的调用堆栈
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            frames: Vec::with_capacity(capacity),
            depth: 0,
            is_complete: true,
        }
    }

    /// 添加堆栈帧到栈顶
    pub fn push(&mut self, frame: StackFrame) {
        self.frames.push(frame);
        self.depth = self.frames.len();
    }

    /// 从栈顶移除堆栈帧
    pub fn pop(&mut self) -> Option<StackFrame> {
        let frame = self.frames.pop();
        self.depth = self.frames.len();
        frame
    }

    /// 获取栈顶的堆栈帧
    pub fn top(&self) -> Option<&StackFrame> {
        self.frames.first()
    }

    /// 获取栈底的堆栈帧
    pub fn bottom(&self) -> Option<&StackFrame> {
        self.frames.last()
    }

    /// 标记堆栈被截断
    pub fn mark_incomplete(&mut self) {
        self.is_complete = false;
    }

    /// 截断堆栈到指定深度
    pub fn truncate(&mut self, max_depth: usize) {
        if self.frames.len() > max_depth {
            self.frames.truncate(max_depth);
            self.depth = max_depth;
            self.is_complete = false;
        }
    }

    /// 获取特定深度的堆栈帧
    pub fn get(&self, index: usize) -> Option<&StackFrame> {
        self.frames.get(index)
    }

    /// 迭代堆栈帧
    pub fn iter(&self) -> impl Iterator<Item = &StackFrame> {
        self.frames.iter()
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// 获取堆栈帧数量
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// 转换为火焰图格式字符串
    pub fn to_flame_graph_string(&self) -> String {
        self.frames
            .iter()
            .rev()
            .map(|f| f.short_name())
            .collect::<Vec<_>>()
            .join(";")
    }

    /// 获取简化表示（仅包含地址）
    pub fn to_address_list(&self) -> Vec<Address> {
        self.frames.iter().map(|f| f.address).collect()
    }
}

impl IntoIterator for CallStack {
    type Item = StackFrame;
    type IntoIter = std::vec::IntoIter<StackFrame>;

    fn into_iter(self) -> Self::IntoIter {
        self.frames.into_iter()
    }
}

impl<'a> IntoIterator for &'a CallStack {
    type Item = &'a StackFrame;
    type IntoIter = std::slice::Iter<'a, StackFrame>;

    fn into_iter(self) -> Self::IntoIter {
        self.frames.iter()
    }
}

// ============================================================================
// 函数统计
// ============================================================================

/// 函数统计信息
///
/// 记录单个函数的统计信息，包括总耗时、调用次数等。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionStats {
    /// 函数完整名称（含模块）
    pub function_name: String,
    /// 模块名称
    pub module_name: Option<String>,
    /// 总耗时（微秒）
    pub total_time_us: u64,
    /// 自身耗时（微秒，不包括子函数）
    pub self_time_us: u64,
    /// 调用次数
    pub call_count: u64,
    /// 平均耗时（微秒）
    pub average_time_us: f64,
    /// 最小耗时（微秒）
    pub min_time_us: u64,
    /// 最大耗时（微秒）
    pub max_time_us: u64,
    /// 关联的线程 ID
    pub thread_id: Option<ThreadId>,
}

impl FunctionStats {
    /// 创建新的函数统计
    pub fn new(function_name: impl Into<String>) -> Self {
        Self {
            function_name: function_name.into(),
            module_name: None,
            total_time_us: 0,
            self_time_us: 0,
            call_count: 0,
            average_time_us: 0.0,
            min_time_us: u64::MAX,
            max_time_us: 0,
            thread_id: None,
        }
    }

    /// 设置模块名称
    pub fn with_module(mut self, module: impl Into<String>) -> Self {
        self.module_name = Some(module.into());
        self
    }

    /// 设置线程 ID
    pub fn with_thread(mut self, thread_id: ThreadId) -> Self {
        self.thread_id = Some(thread_id);
        self
    }

    /// 添加一次调用样本
    ///
    /// # 参数
    /// - `duration_us`: 本次调用的持续时间（微秒）
    /// - `self_duration_us`: 自身执行时间（不包括子函数，微秒）
    pub fn add_sample(&mut self, duration_us: u64, self_duration_us: u64) {
        self.total_time_us += duration_us;
        self.self_time_us += self_duration_us;
        self.call_count += 1;
        self.min_time_us = self.min_time_us.min(duration_us);
        self.max_time_us = self.max_time_us.max(duration_us);
        self.average_time_us = self.total_time_us as f64 / self.call_count as f64;
    }

    /// 合并另一个函数统计
    pub fn merge(&mut self, other: &FunctionStats) {
        if self.function_name != other.function_name {
            return;
        }

        self.total_time_us += other.total_time_us;
        self.self_time_us += other.self_time_us;
        self.call_count += other.call_count;
        self.min_time_us = self.min_time_us.min(other.min_time_us);
        self.max_time_us = self.max_time_us.max(other.max_time_us);

        if self.call_count > 0 {
            self.average_time_us = self.total_time_us as f64 / self.call_count as f64;
        }
    }

    /// 获取耗时百分比
    pub fn time_percentage(&self, total_time: u64) -> f64 {
        if total_time == 0 {
            0.0
        } else {
            (self.total_time_us as f64 / total_time as f64) * 100.0
        }
    }

    /// 获取函数名（不含模块）
    pub fn simple_name(&self) -> &str {
        match self.function_name.rfind('!') {
            Some(pos) => &self.function_name[pos + 1..],
            None => &self.function_name,
        }
    }
}

impl Default for FunctionStats {
    fn default() -> Self {
        Self {
            function_name: String::new(),
            module_name: None,
            total_time_us: 0,
            self_time_us: 0,
            call_count: 0,
            average_time_us: 0.0,
            min_time_us: 0,
            max_time_us: 0,
            thread_id: None,
        }
    }
}

// ============================================================================
// 线程统计
// ============================================================================

/// 线程统计信息
///
/// 记录单个线程的统计信息，包括采样总数和函数统计映射。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThreadStats {
    /// 线程 ID
    pub thread_id: ThreadId,
    /// 所属进程 ID
    pub process_id: ProcessId,
    /// 总采样数
    pub total_samples: u64,
    /// 函数统计映射（函数名 -> 统计信息）
    pub function_stats: HashMap<String, FunctionStats>,
    /// 线程总执行时间（微秒）
    pub total_execution_time_us: u64,
    /// 线程创建时间戳
    pub start_timestamp: Option<Timestamp>,
    /// 线程结束时间戳
    pub end_timestamp: Option<Timestamp>,
}

impl ThreadStats {
    /// 创建新的线程统计
    pub fn new(thread_id: ThreadId, process_id: ProcessId) -> Self {
        Self {
            thread_id,
            process_id,
            total_samples: 0,
            function_stats: HashMap::new(),
            total_execution_time_us: 0,
            start_timestamp: None,
            end_timestamp: None,
        }
    }

    /// 记录采样
    pub fn record_sample(&mut self, stack: &CallStack) {
        self.total_samples += 1;

        // 更新栈顶函数的统计
        if let Some(frame) = stack.top() {
            let func_name = frame.full_function_name();
            let stats = self
                .function_stats
                .entry(func_name.clone())
                .or_insert_with(|| {
                    FunctionStats::new(func_name)
                        .with_thread(self.thread_id)
                        .with_module(frame.module_name.clone().unwrap_or_default())
                });

            // 假设每次采样代表 1ms
            stats.add_sample(1000, 1000);
        }
    }

    /// 添加函数统计
    pub fn add_function_stats(&mut self, stats: FunctionStats) {
        let entry = self
            .function_stats
            .entry(stats.function_name.clone())
            .or_insert_with(|| FunctionStats::new(&stats.function_name).with_thread(self.thread_id));

        entry.merge(&stats);
    }

    /// 获取指定函数的统计
    pub fn get_function_stats(&self, function_name: &str) -> Option<&FunctionStats> {
        self.function_stats.get(function_name)
    }

    /// 获取最耗时的函数列表（按总耗时排序）
    pub fn top_functions_by_time(&self, limit: usize) -> Vec<&FunctionStats> {
        let mut functions: Vec<&FunctionStats> = self.function_stats.values().collect();
        functions.sort_by(|a, b| b.total_time_us.cmp(&a.total_time_us));
        functions.into_iter().take(limit).collect()
    }

    /// 获取调用次数最多的函数列表
    pub fn top_functions_by_calls(&self, limit: usize) -> Vec<&FunctionStats> {
        let mut functions: Vec<&FunctionStats> = self.function_stats.values().collect();
        functions.sort_by(|a, b| b.call_count.cmp(&a.call_count));
        functions.into_iter().take(limit).collect()
    }

    /// 计算 CPU 使用率（基于采样数）
    pub fn cpu_usage_percent(&self, total_system_samples: u64) -> f64 {
        if total_system_samples == 0 {
            0.0
        } else {
            (self.total_samples as f64 / total_system_samples as f64) * 100.0
        }
    }

    /// 获取函数数量
    pub fn function_count(&self) -> usize {
        self.function_stats.len()
    }

    /// 标记线程开始
    pub fn mark_started(&mut self, timestamp: Timestamp) {
        self.start_timestamp = Some(timestamp);
    }

    /// 标记线程结束
    pub fn mark_ended(&mut self, timestamp: Timestamp) {
        self.end_timestamp = Some(timestamp);
        if let Some(start) = self.start_timestamp {
            self.total_execution_time_us = timestamp.saturating_sub(start);
        }
    }

    /// 获取线程存活时间（微秒）
    pub fn lifetime_us(&self) -> u64 {
        match (self.start_timestamp, self.end_timestamp) {
            (Some(start), Some(end)) => end.saturating_sub(start),
            (Some(start), None) => {
                // 假设当前时间
                start.saturating_sub(start)
            }
            _ => 0,
        }
    }
}

impl Default for ThreadStats {
    fn default() -> Self {
        Self {
            thread_id: 0,
            process_id: 0,
            total_samples: 0,
            function_stats: HashMap::new(),
            total_execution_time_us: 0,
            start_timestamp: None,
            end_timestamp: None,
        }
    }
}

// ============================================================================
// 进程统计
// ============================================================================

/// 进程统计信息
///
/// 记录整个进程的统计信息。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ProcessStats {
    /// 进程 ID
    pub process_id: ProcessId,
    /// 进程名称
    pub process_name: Option<String>,
    /// 线程统计映射
    pub thread_stats: HashMap<ThreadId, ThreadStats>,
    /// 总采样数
    pub total_samples: u64,
    /// 采样开始时间
    pub profile_start_time: Option<Timestamp>,
    /// 采样结束时间
    pub profile_end_time: Option<Timestamp>,
}

impl ProcessStats {
    /// 创建新的进程统计
    pub fn new(process_id: ProcessId) -> Self {
        Self {
            process_id,
            process_name: None,
            thread_stats: HashMap::new(),
            total_samples: 0,
            profile_start_time: None,
            profile_end_time: None,
        }
    }

    /// 设置进程名称
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.process_name = Some(name.into());
        self
    }

    /// 记录采样事件
    pub fn record_sample(&mut self, event: &SampleEvent, stack: &CallStack) {
        self.total_samples += 1;

        let thread_stats = self
            .thread_stats
            .entry(event.thread_id)
            .or_insert_with(|| ThreadStats::new(event.thread_id, self.process_id));

        thread_stats.record_sample(stack);
    }

    /// 获取指定线程的统计
    pub fn get_thread_stats(&self, thread_id: ThreadId) -> Option<&ThreadStats> {
        self.thread_stats.get(&thread_id)
    }

    /// 获取可变的线程统计
    pub fn get_thread_stats_mut(&mut self, thread_id: ThreadId) -> Option<&mut ThreadStats> {
        self.thread_stats.get_mut(&thread_id)
    }

    /// 获取所有线程的函数统计（聚合）
    pub fn aggregate_function_stats(&self) -> HashMap<String, FunctionStats> {
        let mut aggregated: HashMap<String, FunctionStats> = HashMap::new();

        for thread in self.thread_stats.values() {
            for (func_name, stats) in &thread.function_stats {
                let entry = aggregated
                    .entry(func_name.clone())
                    .or_insert_with(|| FunctionStats::new(func_name.clone()));
                entry.merge(stats);
            }
        }

        aggregated
    }

    /// 获取最活跃线程列表
    pub fn top_threads(&self, limit: usize) -> Vec<&ThreadStats> {
        let mut threads: Vec<&ThreadStats> = self.thread_stats.values().collect();
        threads.sort_by(|a, b| b.total_samples.cmp(&a.total_samples));
        threads.into_iter().take(limit).collect()
    }

    /// 获取线程数量
    pub fn thread_count(&self) -> usize {
        self.thread_stats.len()
    }

    /// 获取采样持续时间（微秒）
    pub fn duration_us(&self) -> u64 {
        match (self.profile_start_time, self.profile_end_time) {
            (Some(start), Some(end)) => end.saturating_sub(start),
            _ => 0,
        }
    }
}

// ============================================================================
// 辅助类型
// ============================================================================

/// 模块信息
///
/// 描述已加载的模块（DLL/EXE）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleInfo {
    /// 模块基地址
    pub base_address: Address,
    /// 模块大小
    pub size: u64,
    /// 模块名称
    pub name: String,
    /// 模块完整路径
    pub path: Option<String>,
    /// PDB 文件路径
    pub pdb_path: Option<String>,
    /// 时间戳
    pub timestamp: u32,
    /// 校验和
    pub checksum: u32,
}

impl ModuleInfo {
    /// 创建新的模块信息
    pub fn new(base_address: Address, size: u64, name: impl Into<String>) -> Self {
        Self {
            base_address,
            size,
            name: name.into(),
            path: None,
            pdb_path: None,
            timestamp: 0,
            checksum: 0,
        }
    }

    /// 检查地址是否在此模块范围内
    pub fn contains_address(&self, address: Address) -> bool {
        address >= self.base_address && address < self.base_address + self.size
    }
}

/// 符号信息
///
/// 描述解析后的符号。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SymbolInfo {
    /// 符号地址
    pub address: Address,
    /// 符号名称
    pub name: String,
    /// 所属模块
    pub module: Option<String>,
    /// 源文件
    pub source_file: Option<String>,
    /// 行号
    pub line_number: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_event() {
        let event = SampleEvent::new(1000, 1234, 5678, 0x00400000)
            .with_processor_core(0)
            .with_priority(8);

        assert_eq!(event.timestamp, 1000);
        assert_eq!(event.process_id, 1234);
        assert_eq!(event.thread_id, 5678);
        assert_eq!(event.instruction_pointer, 0x00400000);
        assert_eq!(event.processor_core, Some(0));
        assert_eq!(event.priority, Some(8));
        assert!(event.is_process(1234));
        assert!(!event.is_process(9999));
    }

    #[test]
    fn test_stack_frame() {
        let frame = StackFrame::new(0x00401000)
            .with_module("test.dll")
            .with_function("TestFunction")
            .with_source("test.cpp", 42, Some(10));

        assert_eq!(frame.address, 0x00401000);
        assert_eq!(frame.module_name, Some("test.dll".to_string()));
        assert_eq!(frame.function_name, Some("TestFunction".to_string()));
        assert_eq!(frame.file_name, Some("test.cpp".to_string()));
        assert_eq!(frame.line_number, Some(42));
        assert!(frame.has_symbol_info());
        assert!(frame.has_source_info());
        assert_eq!(frame.full_function_name(), "test.dll!TestFunction");
    }

    #[test]
    fn test_call_stack() {
        let mut stack = CallStack::new();
        stack.push(StackFrame::new(0x001));
        stack.push(StackFrame::new(0x002));
        stack.push(StackFrame::new(0x003));

        assert_eq!(stack.len(), 3);
        assert_eq!(stack.top().unwrap().address, 0x001);
        assert_eq!(stack.bottom().unwrap().address, 0x003);

        stack.truncate(2);
        assert_eq!(stack.len(), 2);
        assert!(!stack.is_complete);
    }

    #[test]
    fn test_function_stats() {
        let mut stats = FunctionStats::new("test.dll!TestFunction").with_module("test.dll");

        stats.add_sample(1000, 800);
        stats.add_sample(2000, 1500);

        assert_eq!(stats.call_count, 2);
        assert_eq!(stats.total_time_us, 3000);
        assert_eq!(stats.self_time_us, 2300);
        assert_eq!(stats.min_time_us, 1000);
        assert_eq!(stats.max_time_us, 2000);
        assert_eq!(stats.average_time_us, 1500.0);
        assert_eq!(stats.simple_name(), "TestFunction");
    }

    #[test]
    fn test_thread_stats() {
        let mut thread_stats = ThreadStats::new(1234, 5678);
        assert_eq!(thread_stats.thread_id, 1234);
        assert_eq!(thread_stats.process_id, 5678);

        let stack = CallStack {
            frames: vec![StackFrame::with_symbol(
                0x00401000,
                "test.dll",
                "TestFunction",
            )],
            depth: 1,
            is_complete: true,
        };

        thread_stats.record_sample(&stack);
        assert_eq!(thread_stats.total_samples, 1);
        assert_eq!(thread_stats.function_count(), 1);

        let func_stats = thread_stats.get_function_stats("test.dll!TestFunction");
        assert!(func_stats.is_some());
        assert_eq!(func_stats.unwrap().call_count, 1);
    }

    #[test]
    fn test_module_info() {
        let module = ModuleInfo::new(0x00400000, 0x10000, "test.exe");
        assert!(module.contains_address(0x00401000));
        assert!(!module.contains_address(0x00500000));
    }
}
