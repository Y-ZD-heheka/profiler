//! 统计聚合器模块
//!
//! 提供性能分析数据的统计聚合功能，按线程分类处理堆栈数据，
//! 计算每个函数的耗时统计信息。

use crate::error::Result;
use crate::stackwalker::{is_system_module, CollectedStack, ResolvedFrame};
use crate::types::{FunctionStats, ThreadId, ThreadStats, ProcessId};

use std::collections::HashMap;
use tracing::{debug, trace, warn};

/// 聚合器配置
///
/// 控制统计聚合的行为和选项。
#[derive(Debug, Clone)]
pub struct AggregatorConfig {
    /// 是否合并递归调用
    pub merge_recursive_calls: bool,
    /// 是否排除系统函数
    pub exclude_system_functions: bool,
    /// 最小样本数阈值
    pub min_sample_count: usize,
    /// 采样间隔（毫秒）
    pub sample_interval_ms: u32,
    /// 最大堆栈深度
    pub max_stack_depth: usize,
    /// 是否计算自身耗时
    pub calculate_self_time: bool,
}

impl Default for AggregatorConfig {
    fn default() -> Self {
        Self {
            merge_recursive_calls: true,
            exclude_system_functions: false,
            min_sample_count: 1,
            sample_interval_ms: 1,
            max_stack_depth: 128,
            calculate_self_time: true,
        }
    }
}

impl AggregatorConfig {
    /// 创建新的聚合器配置
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置是否合并递归调用
    pub fn with_merge_recursive_calls(mut self, merge: bool) -> Self {
        self.merge_recursive_calls = merge;
        self
    }

    /// 设置是否排除系统函数
    pub fn with_exclude_system_functions(mut self, exclude: bool) -> Self {
        self.exclude_system_functions = exclude;
        self
    }

    /// 设置最小样本数阈值
    pub fn with_min_sample_count(mut self, count: usize) -> Self {
        self.min_sample_count = count;
        self
    }

    /// 设置采样间隔
    pub fn with_sample_interval_ms(mut self, interval_ms: u32) -> Self {
        self.sample_interval_ms = interval_ms;
        self
    }
}

/// 统计聚合器
///
/// 处理收集到的堆栈数据，按线程分类，计算每个函数的耗时统计信息。
#[derive(Debug)]
pub struct StatsAggregator {
    /// 聚合器配置
    config: AggregatorConfig,
    /// 线程统计映射（线程ID -> 线程统计）
    thread_stats: HashMap<ThreadId, ThreadStats>,
    /// 进程统计
    process_stats: HashMap<ProcessId, u64>,
    /// 总样本数
    total_samples: u64,
    /// 总采样时间（毫秒）
    total_time_ms: u64,
    /// 递归调用检测器
    recursion_detector: RecursionDetector,
}

/// 递归调用检测器
#[derive(Debug, Default)]
struct RecursionDetector {
    /// 记录每个线程的调用链
    call_chains: HashMap<ThreadId, Vec<String>>,
}

impl RecursionDetector {
    /// 检测并压缩递归调用
    fn detect_and_compress(&mut self, thread_id: ThreadId, stack: &CollectedStack) -> Vec<String> {
        let frames: Vec<String> = stack
            .frames
            .iter()
            .map(|f| f.display_name())
            .collect();

        if !self.call_chains.contains_key(&thread_id) {
            self.call_chains.insert(thread_id, Vec::new());
        }

        // 简单的递归检测：检查是否有连续的重复函数
        let mut compressed: Vec<String> = Vec::new();
        let mut last_func: Option<&str> = None;

        for func in &frames {
            if let Some(last) = last_func {
                if last == func {
                    // 检测到递归调用，跳过
                    continue;
                }
            }
            compressed.push(func.clone());
            last_func = Some(func);
        }

        self.call_chains.insert(thread_id, compressed.clone());
        compressed
    }

    /// 清除线程的调用链记录
    fn clear_thread(&mut self, thread_id: ThreadId) {
        self.call_chains.remove(&thread_id);
    }

    /// 重置所有记录
    fn reset(&mut self) {
        self.call_chains.clear();
    }
}

/// 从 ResolvedFrame 获取模块名称
fn get_module_name(frame: &ResolvedFrame) -> Option<String> {
    frame.module.as_ref().map(|m| m.name.clone())
}

impl StatsAggregator {
    /// 创建新的统计聚合器
    pub fn new(config: AggregatorConfig) -> Self {
        Self {
            config,
            thread_stats: HashMap::new(),
            process_stats: HashMap::new(),
            total_samples: 0,
            total_time_ms: 0,
            recursion_detector: RecursionDetector::default(),
        }
    }

    /// 处理单个堆栈
    pub fn process_stack(&mut self, stack: &CollectedStack) -> Result<()> {
        trace!(
            "Processing stack for thread {} in process {}",
            stack.thread_id,
            stack.process_id
        );

        // 检查堆栈帧数量
        if stack.frames.is_empty() {
            warn!(
                "Empty stack received for thread {} in process {}",
                stack.thread_id, stack.process_id
            );
            return Ok(());
        }

        // 获取或创建线程统计
        let thread_stats = self
            .thread_stats
            .entry(stack.thread_id)
            .or_insert_with(|| ThreadStats::new(stack.thread_id, stack.process_id));

        // 更新线程统计
        thread_stats.total_samples += 1;

        // 处理堆栈帧
        let frames_to_process: Vec<String> = if self.config.merge_recursive_calls {
            self.recursion_detector
                .detect_and_compress(stack.thread_id, stack)
        } else {
            stack.frames.iter().map(|f| f.display_name()).collect()
        };

        // 计算每个函数的统计信息
        let sample_interval_us = self.config.sample_interval_ms as u64 * 1000;
        let frame_count = frames_to_process.len();

        for (depth, func_name) in frames_to_process.iter().enumerate() {
            // 检查是否应该排除系统函数
            if self.config.exclude_system_functions {
                if let Some(frame) = stack.frames.get(depth) {
                    if let Some(ref module) = frame.module {
                        if is_system_module(&module.name) {
                            continue;
                        }
                    }
                }
            }

            // 获取或创建函数统计
            let func_stats = thread_stats
                .function_stats
                .entry(func_name.clone())
                .or_insert_with(|| {
                    let mut stats = FunctionStats::new(func_name.clone());
                    stats.thread_id = Some(stack.thread_id);
                    if let Some(frame) = stack.frames.get(depth) {
                        stats.module_name = get_module_name(frame);
                    }
                    stats
                });

            // 更新函数统计
            func_stats.total_time_us += sample_interval_us;

            // 自身耗时计算：只有叶节点（栈顶）算自身耗时
            let is_leaf = depth == 0;
            if is_leaf {
                func_stats.self_time_us += sample_interval_us;
                func_stats.call_count += 1;
            }

            // 更新耗时范围
            func_stats.min_time_us = func_stats.min_time_us.min(sample_interval_us);
            func_stats.max_time_us = func_stats.max_time_us.max(sample_interval_us);
        }

        // 更新进程统计
        *self
            .process_stats
            .entry(stack.process_id)
            .or_insert(0) += 1;

        // 更新总样本数
        self.total_samples += 1;
        self.total_time_ms += self.config.sample_interval_ms as u64;

        debug!(
            "Processed stack with {} frames for thread {}",
            frame_count, stack.thread_id
        );

        Ok(())
    }

    /// 批量处理堆栈数据
    pub fn process_stacks(&mut self, stacks: &[CollectedStack]) -> Result<()> {
        for stack in stacks {
            self.process_stack(stack)?;
        }
        Ok(())
    }

    /// 获取指定线程的统计信息
    pub fn get_thread_stats(&self, thread_id: ThreadId) -> Option<&ThreadStats> {
        self.thread_stats.get(&thread_id)
    }

    /// 获取可变的线程统计信息
    pub fn get_thread_stats_mut(&mut self, thread_id: ThreadId) -> Option<&mut ThreadStats> {
        self.thread_stats.get_mut(&thread_id)
    }

    /// 获取所有线程的统计信息
    pub fn get_all_thread_stats(&self) -> &HashMap<ThreadId, ThreadStats> {
        &self.thread_stats
    }

    /// 获取指定线程中指定函数的统计信息
    pub fn get_function_stats(
        &self,
        thread_id: ThreadId,
        function_name: &str,
    ) -> Option<&FunctionStats> {
        self.thread_stats
            .get(&thread_id)
            .and_then(|ts| ts.function_stats.get(function_name))
    }

    /// 获取指定线程中耗时最高的函数列表
    pub fn get_top_functions(&self, thread_id: ThreadId, limit: usize) -> Vec<&FunctionStats> {
        self.thread_stats
            .get(&thread_id)
            .map(|ts| {
                let mut functions: Vec<&FunctionStats> = ts.function_stats.values().collect();
                functions.sort_by(|a, b| b.total_time_us.cmp(&a.total_time_us));
                functions.into_iter().take(limit).collect()
            })
            .unwrap_or_default()
    }

    /// 获取所有线程中耗时最高的函数列表
    pub fn get_global_top_functions(&self, limit: usize) -> Vec<&FunctionStats> {
        let mut all_functions: Vec<&FunctionStats> = self
            .thread_stats
            .values()
            .flat_map(|ts| ts.function_stats.values())
            .collect();

        all_functions.sort_by(|a, b| b.total_time_us.cmp(&a.total_time_us));
        all_functions.into_iter().take(limit).collect()
    }

    /// 获取指定进程的所有线程统计
    pub fn get_process_threads(&self, process_id: ProcessId) -> Vec<&ThreadStats> {
        self.thread_stats
            .values()
            .filter(|ts| ts.process_id == process_id)
            .collect()
    }

    /// 计算所有线程的函数自身耗时
    pub fn calculate_exclusive_times(&mut self) {
        for (thread_id, thread_stats) in &mut self.thread_stats {
            let func_names: Vec<String> = thread_stats.function_stats.keys().cloned().collect();

            for func_name in &func_names {
                if let Some(stats) = thread_stats.function_stats.get(func_name) {
                    let total_time = stats.total_time_us;
                    let self_time = stats.self_time_us;
                    trace!(
                        "Thread {} function {}: total={}μs, self={}μs",
                        thread_id,
                        func_name,
                        total_time,
                        self_time
                    );
                }
            }

            // 更新平均耗时
            for stats in thread_stats.function_stats.values_mut() {
                if stats.call_count > 0 {
                    stats.average_time_us = stats.total_time_us as f64 / stats.call_count as f64;
                }
            }
        }
    }

    /// 计算所有百分比（记录到日志）
    pub fn calculate_percentages(&mut self) {
        let total_time = self.total_time_ms * 1000;

        for thread_stats in self.thread_stats.values_mut() {
            let thread_total = thread_stats.total_samples * self.config.sample_interval_ms as u64 * 1000;

            for stats in thread_stats.function_stats.values_mut() {
                let _total_percentage = if total_time > 0 {
                    (stats.total_time_us as f64 / total_time as f64) * 100.0
                } else {
                    0.0
                };
                let _self_percentage = if thread_total > 0 {
                    (stats.self_time_us as f64 / thread_total as f64) * 100.0
                } else {
                    0.0
                };
                // 百分比可以通过 time_percentage 方法计算
            }
        }
    }

    /// 过滤低样本数函数
    pub fn filter_low_sample_functions(&mut self) {
        let threshold = self.config.min_sample_count as u64;

        for thread_stats in self.thread_stats.values_mut() {
            thread_stats
                .function_stats
                .retain(|_name, stats| stats.call_count >= threshold);
        }
    }

    /// 重置所有统计信息
    pub fn reset(&mut self) {
        self.thread_stats.clear();
        self.process_stats.clear();
        self.total_samples = 0;
        self.total_time_ms = 0;
        self.recursion_detector.reset();
        debug!("StatsAggregator reset completed");
    }

    /// 获取总样本数
    pub fn total_samples(&self) -> u64 {
        self.total_samples
    }

    /// 获取配置引用
    pub fn config(&self) -> &AggregatorConfig {
        &self.config
    }

    /// 获取配置的可变引用
    pub fn config_mut(&mut self) -> &mut AggregatorConfig {
        &mut self.config
    }

    /// 获取线程数量
    pub fn thread_count(&self) -> usize {
        self.thread_stats.len()
    }

    /// 获取进程数量
    pub fn process_count(&self) -> usize {
        self.process_stats.len()
    }

    /// 获取总采样时间（毫秒）
    pub fn total_time_ms(&self) -> u64 {
        self.total_time_ms
    }

    /// 生成汇总报告
    pub fn generate_summary(&self) -> AggregatorSummary {
        let total_functions: usize = self
            .thread_stats
            .values()
            .map(|ts| ts.function_stats.len())
            .sum();

        AggregatorSummary {
            total_samples: self.total_samples,
            total_threads: self.thread_stats.len(),
            total_processes: self.process_stats.len(),
            total_functions,
            total_time_ms: self.total_time_ms,
        }
    }
}

/// 聚合器汇总信息
#[derive(Debug, Clone)]
pub struct AggregatorSummary {
    /// 总样本数
    pub total_samples: u64,
    /// 总线程数
    pub total_threads: usize,
    /// 总进程数
    pub total_processes: usize,
    /// 总函数数
    pub total_functions: usize,
    /// 总采样时间（毫秒）
    pub total_time_ms: u64,
}

impl std::fmt::Display for AggregatorSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== 统计聚合器汇总 ===")?;
        writeln!(f, "总样本数: {}", self.total_samples)?;
        writeln!(f, "总线程数: {}", self.total_threads)?;
        writeln!(f, "总进程数: {}", self.total_processes)?;
        writeln!(f, "总函数数: {}", self.total_functions)?;
        writeln!(f, "总采样时间: {} ms", self.total_time_ms)?;
        writeln!(
            f,
            "平均采样间隔: {:.2} ms",
            if self.total_samples > 0 {
                self.total_time_ms as f64 / self.total_samples as f64
            } else {
                0.0
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stackwalker::FrameType;
    use crate::types::ModuleInfo;

    fn create_test_stack(thread_id: ThreadId, process_id: ProcessId) -> CollectedStack {
        CollectedStack::new(
            process_id,
            thread_id,
            1000,
            vec![
                ResolvedFrame::with_type(0x00401000, FrameType::User)
                    .with_module(ModuleInfo::new(0x00400000, 0x10000, "test.exe")),
                ResolvedFrame::with_type(0x00402000, FrameType::User)
                    .with_module(ModuleInfo::new(0x00400000, 0x10000, "test.exe")),
            ],
        )
    }

    #[test]
    fn test_aggregator_new() {
        let config = AggregatorConfig::default();
        let aggregator = StatsAggregator::new(config);

        assert_eq!(aggregator.total_samples(), 0);
        assert_eq!(aggregator.thread_count(), 0);
    }

    #[test]
    fn test_process_stack() {
        let config = AggregatorConfig::default();
        let mut aggregator = StatsAggregator::new(config);

        let stack = create_test_stack(1, 1000);
        aggregator.process_stack(&stack).unwrap();

        assert_eq!(aggregator.total_samples(), 1);
        assert_eq!(aggregator.thread_count(), 1);

        let thread_stats = aggregator.get_thread_stats(1).unwrap();
        assert_eq!(thread_stats.total_samples, 1);
        assert_eq!(thread_stats.function_stats.len(), 2);
    }

    #[test]
    fn test_get_top_functions() {
        let config = AggregatorConfig::default();
        let mut aggregator = StatsAggregator::new(config);

        for _ in 0..10 {
            let stack = create_test_stack(1, 1000);
            aggregator.process_stack(&stack).unwrap();
        }

        let top_functions = aggregator.get_top_functions(1, 2);
        assert_eq!(top_functions.len(), 2);
    }

    #[test]
    fn test_reset() {
        let config = AggregatorConfig::default();
        let mut aggregator = StatsAggregator::new(config);

        let stack = create_test_stack(1, 1000);
        aggregator.process_stack(&stack).unwrap();

        assert_eq!(aggregator.total_samples(), 1);

        aggregator.reset();

        assert_eq!(aggregator.total_samples(), 0);
        assert_eq!(aggregator.thread_count(), 0);
    }

    #[test]
    fn test_config_builder() {
        let config = AggregatorConfig::new()
            .with_merge_recursive_calls(false)
            .with_exclude_system_functions(true)
            .with_min_sample_count(5)
            .with_sample_interval_ms(10);

        assert!(!config.merge_recursive_calls);
        assert!(config.exclude_system_functions);
        assert_eq!(config.min_sample_count, 5);
        assert_eq!(config.sample_interval_ms, 10);
    }
}
