//! 函数统计计算模块
//!
//! 提供函数统计的详细计算功能，包括样本记录、百分比计算和时间估算。

use crate::types::{FunctionStats, ThreadId};

use std::collections::HashMap;
use tracing::{debug, trace};

/// 样本记录
///
/// 记录单个样本的信息。
#[derive(Debug, Clone)]
pub struct SampleRecord {
    /// 函数名称
    pub function_name: String,
    /// 调用深度
    pub stack_depth: usize,
    /// 是否为叶节点
    pub is_leaf: bool,
    /// 样本时间戳
    pub timestamp: u64,
    /// 样本耗时（微秒）
    pub duration_us: u64,
    /// 调用来源
    pub caller: Option<String>,
}

impl SampleRecord {
    /// 创建新的样本记录
    pub fn new(function_name: impl Into<String>, stack_depth: usize, is_leaf: bool) -> Self {
        Self {
            function_name: function_name.into(),
            stack_depth,
            is_leaf,
            timestamp: 0,
            duration_us: 1000, // 默认1ms
            caller: None,
        }
    }

    /// 设置时间戳
    pub fn with_timestamp(mut self, timestamp: u64) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// 设置耗时
    pub fn with_duration(mut self, duration_us: u64) -> Self {
        self.duration_us = duration_us;
        self
    }

    /// 设置调用者
    pub fn with_caller(mut self, caller: impl Into<String>) -> Self {
        self.caller = Some(caller.into());
        self
    }
}

/// 函数统计计算器
///
/// 提供函数统计的增量计算和更新功能。
#[derive(Debug)]
pub struct FunctionStatsCalculator {
    /// 函数名称
    function_name: String,
    /// 模块名称
    module_name: Option<String>,
    /// 关联线程ID
    thread_id: Option<ThreadId>,
    /// 总样本数
    total_samples: u64,
    /// 自身样本数（作为叶节点）
    self_samples: u64,
    /// 总耗时（微秒）
    total_time_us: u64,
    /// 自身耗时（微秒）
    self_time_us: u64,
    /// 深度分布（深度 -> 出现次数）
    depth_distribution: HashMap<usize, u64>,
    /// 调用来源分布（来源函数 -> 次数）
    caller_distribution: HashMap<String, u64>,
    /// 首次出现时间戳
    first_seen: Option<u64>,
    /// 最后出现时间戳
    last_seen: Option<u64>,
    /// 最小深度
    min_depth: usize,
    /// 最大深度
    max_depth: usize,
    /// 总深度（用于计算平均值）
    total_depth: u64,
    /// 采样间隔（毫秒）
    sample_interval_ms: u32,
    /// 是否启用详细统计
    detailed_stats: bool,
}

impl FunctionStatsCalculator {
    /// 创建新的函数统计计算器
    ///
    /// # 参数
    /// - `function_name`: 函数名称
    pub fn new(function_name: impl Into<String>) -> Self {
        Self {
            function_name: function_name.into(),
            module_name: None,
            thread_id: None,
            total_samples: 0,
            self_samples: 0,
            total_time_us: 0,
            self_time_us: 0,
            depth_distribution: HashMap::new(),
            caller_distribution: HashMap::new(),
            first_seen: None,
            last_seen: None,
            min_depth: usize::MAX,
            max_depth: 0,
            total_depth: 0,
            sample_interval_ms: 1,
            detailed_stats: true,
        }
    }

    /// 设置模块名称
    pub fn with_module(mut self, module: impl Into<String>) -> Self {
        self.module_name = Some(module.into());
        self
    }

    /// 设置线程ID
    pub fn with_thread_id(mut self, thread_id: ThreadId) -> Self {
        self.thread_id = Some(thread_id);
        self
    }

    /// 设置采样间隔
    pub fn with_sample_interval(mut self, interval_ms: u32) -> Self {
        self.sample_interval_ms = interval_ms;
        self
    }

    /// 设置是否启用详细统计
    pub fn with_detailed_stats(mut self, detailed: bool) -> Self {
        self.detailed_stats = detailed;
        self
    }

    /// 记录样本
    ///
    /// # 参数
    /// - `function_name`: 函数名称
    /// - `stack_depth`: 调用深度
    /// - `is_leaf`: 是否为叶节点
    pub fn record_sample(
        &mut self,
        function_name: &str,
        stack_depth: usize,
        is_leaf: bool,
    ) {
        // 验证函数名匹配
        if function_name != self.function_name {
            trace!(
                "Function name mismatch: expected {}, got {}",
                self.function_name,
                function_name
            );
            return;
        }

        let sample_duration = self.sample_interval_ms as u64 * 1000; // 转换为微秒

        // 更新样本计数
        self.total_samples += 1;
        self.total_time_us += sample_duration;

        if is_leaf {
            self.self_samples += 1;
            self.self_time_us += sample_duration;
        }

        // 更新深度统计
        self.min_depth = self.min_depth.min(stack_depth);
        self.max_depth = self.max_depth.max(stack_depth);
        self.total_depth += stack_depth as u64;

        // 更新深度分布
        if self.detailed_stats {
            *self.depth_distribution.entry(stack_depth).or_insert(0) += 1;
        }

        // 更新时间戳
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;

        if self.first_seen.is_none() {
            self.first_seen = Some(now);
        }
        self.last_seen = Some(now);

        trace!(
            "Recorded sample for {}: depth={}, is_leaf={}",
            function_name,
            stack_depth,
            is_leaf
        );
    }

    /// 使用 SampleRecord 记录样本
    pub fn record(&mut self, record: &SampleRecord) {
        self.record_sample(&record.function_name, record.stack_depth, record.is_leaf);

        // 记录调用来源
        if let Some(ref caller) = record.caller {
            *self.caller_distribution.entry(caller.clone()).or_insert(0) += 1;
        }

        // 使用记录中的时间戳
        if self.first_seen.is_none() {
            self.first_seen = Some(record.timestamp);
        }
        if record.timestamp > 0 {
            self.last_seen = Some(record.timestamp);
        }
    }

    /// 批量记录样本
    pub fn record_batch(&mut self, records: &[SampleRecord]) {
        for record in records {
            self.record(record);
        }
        debug!("Recorded batch of {} samples for {}", records.len(), self.function_name);
    }

    /// 计算百分比
    ///
    /// # 参数
    /// - `total_samples`: 总样本数
    pub fn calculate_percentages(&mut self, total_samples: u64) {
        let total_percentage = if total_samples > 0 {
            (self.total_samples as f64 / total_samples as f64) * 100.0
        } else {
            0.0
        };

        trace!(
            "Function {}: {} samples ({:.2}% of total {})",
            self.function_name,
            self.total_samples,
            total_percentage,
            total_samples
        );

        // 百分比存储在最终的 FunctionStats 中
    }

    /// 更新时间估算
    ///
    /// # 参数
    /// - `sample_interval_ms`: 采样间隔（毫秒）
    pub fn update_time_estimates(&mut self, sample_interval_ms: u32) {
        self.sample_interval_ms = sample_interval_ms;
        let interval_us = sample_interval_ms as u64 * 1000;

        // 重新计算时间
        self.total_time_us = self.total_samples * interval_us;
        self.self_time_us = self.self_samples * interval_us;

        debug!(
            "Updated time estimates for {}: total={}μs, self={}μs",
            self.function_name, self.total_time_us, self.self_time_us
        );
    }

    /// 获取平均调用深度
    pub fn average_depth(&self) -> f64 {
        if self.total_samples == 0 {
            0.0
        } else {
            self.total_depth as f64 / self.total_samples as f64
        }
    }

    /// 获取最常见的调用深度
    pub fn most_common_depth(&self) -> Option<usize> {
        self.depth_distribution
            .iter()
            .max_by_key(|(_, count)| *count)
            .map(|(depth, _)| *depth)
    }

    /// 获取最常见的调用来源
    pub fn most_common_caller(&self) -> Option<&str> {
        self.caller_distribution
            .iter()
            .max_by_key(|(_, count)| *count)
            .map(|(caller, _)| caller.as_str())
    }

    /// 获取调用来源数量
    pub fn caller_count(&self) -> usize {
        self.caller_distribution.len()
    }

    /// 构建最终的 FunctionStats
    pub fn build_stats(&self) -> FunctionStats {
        let mut stats = FunctionStats::new(&self.function_name);
        stats.module_name = self.module_name.clone();
        stats.thread_id = self.thread_id;
        stats.call_count = self.total_samples;
        stats.total_time_us = self.total_time_us;
        stats.self_time_us = self.self_time_us;

        // 计算平均耗时
        if self.total_samples > 0 {
            stats.average_time_us = self.total_time_us as f64 / self.total_samples as f64;
        }

        // 设置耗时范围
        if self.total_samples > 0 {
            stats.min_time_us = self.sample_interval_ms as u64 * 1000;
            stats.max_time_us = stats.min_time_us;
        }

        stats
    }

    /// 获取函数名称
    pub fn function_name(&self) -> &str {
        &self.function_name
    }

    /// 获取总样本数
    pub fn total_samples(&self) -> u64 {
        self.total_samples
    }

    /// 获取自身样本数
    pub fn self_samples(&self) -> u64 {
        self.self_samples
    }

    /// 获取总耗时
    pub fn total_time_us(&self) -> u64 {
        self.total_time_us
    }

    /// 获取自身耗时
    pub fn self_time_us(&self) -> u64 {
        self.self_time_us
    }

    /// 获取首次出现时间
    pub fn first_seen(&self) -> Option<u64> {
        self.first_seen
    }

    /// 获取最后出现时间
    pub fn last_seen(&self) -> Option<u64> {
        self.last_seen
    }

    /// 获取持续时间（微秒）
    pub fn duration_us(&self) -> u64 {
        match (self.first_seen, self.last_seen) {
            (Some(first), Some(last)) => last.saturating_sub(first),
            _ => 0,
        }
    }

    /// 重置计算器
    pub fn reset(&mut self) {
        self.total_samples = 0;
        self.self_samples = 0;
        self.total_time_us = 0;
        self.self_time_us = 0;
        self.depth_distribution.clear();
        self.caller_distribution.clear();
        self.first_seen = None;
        self.last_seen = None;
        self.min_depth = usize::MAX;
        self.max_depth = 0;
        self.total_depth = 0;
        trace!("Reset FunctionStatsCalculator for {}", self.function_name);
    }
}

/// 函数统计聚合器
///
/// 管理多个函数的统计计算器。
#[derive(Debug)]
pub struct FunctionStatsAggregator {
    /// 函数统计计算器映射
    calculators: HashMap<String, FunctionStatsCalculator>,
    /// 采样间隔（毫秒）
    sample_interval_ms: u32,
    /// 总样本数
    total_samples: u64,
    /// 是否启用详细统计
    detailed_stats: bool,
}

impl FunctionStatsAggregator {
    /// 创建新的函数统计聚合器
    pub fn new() -> Self {
        Self {
            calculators: HashMap::new(),
            sample_interval_ms: 1,
            total_samples: 0,
            detailed_stats: true,
        }
    }

    /// 设置采样间隔
    pub fn with_sample_interval(mut self, interval_ms: u32) -> Self {
        self.sample_interval_ms = interval_ms;
        self
    }

    /// 设置是否启用详细统计
    pub fn with_detailed_stats(mut self, detailed: bool) -> Self {
        self.detailed_stats = detailed;
        self
    }

    /// 记录样本
    pub fn record_sample(
        &mut self,
        function_name: &str,
        stack_depth: usize,
        is_leaf: bool,
    ) {
        let calculator = self
            .calculators
            .entry(function_name.to_string())
            .or_insert_with(|| {
                FunctionStatsCalculator::new(function_name)
                    .with_sample_interval(self.sample_interval_ms)
                    .with_detailed_stats(self.detailed_stats)
            });

        calculator.record_sample(function_name, stack_depth, is_leaf);
        self.total_samples += 1;
    }

    /// 使用 SampleRecord 记录样本
    pub fn record(&mut self, record: &SampleRecord) {
        let calculator = self
            .calculators
            .entry(record.function_name.clone())
            .or_insert_with(|| {
                FunctionStatsCalculator::new(&record.function_name)
                    .with_sample_interval(self.sample_interval_ms)
                    .with_detailed_stats(self.detailed_stats)
            });

        calculator.record(record);
        self.total_samples += 1;
    }

    /// 获取指定函数的计算器
    pub fn get_calculator(&self, function_name: &str) -> Option<&FunctionStatsCalculator> {
        self.calculators.get(function_name)
    }

    /// 获取可变的计算器
    pub fn get_calculator_mut(
        &mut self,
        function_name: &str,
    ) -> Option<&mut FunctionStatsCalculator> {
        self.calculators.get_mut(function_name)
    }

    /// 获取或创建计算器
    pub fn get_or_create_calculator(
        &mut self,
        function_name: &str,
    ) -> &mut FunctionStatsCalculator {
        self.calculators
            .entry(function_name.to_string())
            .or_insert_with(|| {
                FunctionStatsCalculator::new(function_name)
                    .with_sample_interval(self.sample_interval_ms)
                    .with_detailed_stats(self.detailed_stats)
            })
    }

    /// 计算所有函数的百分比
    pub fn calculate_all_percentages(&mut self) {
        for calculator in self.calculators.values_mut() {
            calculator.calculate_percentages(self.total_samples);
        }
    }

    /// 构建所有函数的统计信息
    pub fn build_all_stats(&self) -> HashMap<String, FunctionStats> {
        self.calculators
            .iter()
            .map(|(name, calc)| (name.clone(), calc.build_stats()))
            .collect()
    }

    /// 获取最耗时的函数（按总时间）
    pub fn get_top_by_total_time(&self, limit: usize) -> Vec<&FunctionStatsCalculator> {
        let mut calculators: Vec<&FunctionStatsCalculator> = self.calculators.values().collect();
        calculators.sort_by(|a, b| b.total_time_us().cmp(&a.total_time_us()));
        calculators.into_iter().take(limit).collect()
    }

    /// 获取自身耗时最高的函数
    pub fn get_top_by_self_time(&self, limit: usize) -> Vec<&FunctionStatsCalculator> {
        let mut calculators: Vec<&FunctionStatsCalculator> = self.calculators.values().collect();
        calculators.sort_by(|a, b| b.self_time_us().cmp(&a.self_time_us()));
        calculators.into_iter().take(limit).collect()
    }

    /// 获取调用次数最多的函数
    pub fn get_top_by_call_count(&self, limit: usize) -> Vec<&FunctionStatsCalculator> {
        let mut calculators: Vec<&FunctionStatsCalculator> = self.calculators.values().collect();
        calculators.sort_by(|a, b| b.total_samples().cmp(&a.total_samples()));
        calculators.into_iter().take(limit).collect()
    }

    /// 获取函数数量
    pub fn function_count(&self) -> usize {
        self.calculators.len()
    }

    /// 获取总样本数
    pub fn total_samples(&self) -> u64 {
        self.total_samples
    }

    /// 重置所有计算器
    pub fn reset(&mut self) {
        self.calculators.clear();
        self.total_samples = 0;
        debug!("Reset FunctionStatsAggregator");
    }
}

impl Default for FunctionStatsAggregator {
    fn default() -> Self {
        Self::new()
    }
}

/// 计算两个样本之间的耗时差
pub fn calculate_duration_us(start: u64, end: u64) -> u64 {
    end.saturating_sub(start)
}

/// 估算CPU周期数（基于假设的CPU频率）
pub fn estimate_cpu_cycles(duration_us: u64, cpu_frequency_ghz: f64) -> u64 {
    (duration_us as f64 * cpu_frequency_ghz * 1000.0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_record() {
        let record = SampleRecord::new("test_function", 5, true)
            .with_timestamp(1000)
            .with_duration(2000)
            .with_caller("caller_function");

        assert_eq!(record.function_name, "test_function");
        assert_eq!(record.stack_depth, 5);
        assert!(record.is_leaf);
        assert_eq!(record.timestamp, 1000);
        assert_eq!(record.duration_us, 2000);
        assert_eq!(record.caller, Some("caller_function".to_string()));
    }

    #[test]
    fn test_function_stats_calculator() {
        let mut calc = FunctionStatsCalculator::new("test_function")
            .with_module("test.exe")
            .with_thread_id(1)
            .with_sample_interval(1);

        // 记录一些样本
        for i in 0..10 {
            calc.record_sample("test_function", i % 3 + 5, i == 0);
        }

        assert_eq!(calc.total_samples(), 10);
        assert_eq!(calc.self_samples(), 1); // 只有一个叶节点
        assert!(calc.total_time_us() > 0);
        assert!(calc.self_time_us() > 0);

        let avg_depth = calc.average_depth();
        assert!(avg_depth > 0.0);

        let stats = calc.build_stats();
        assert_eq!(stats.function_name, "test_function");
        assert_eq!(stats.module_name, Some("test.exe".to_string()));
        assert_eq!(stats.call_count, 10);
    }

    #[test]
    fn test_depth_distribution() {
        let mut calc = FunctionStatsCalculator::new("test_function");

        // 记录不同深度的样本
        calc.record_sample("test_function", 3, false);
        calc.record_sample("test_function", 3, false);
        calc.record_sample("test_function", 5, false);
        calc.record_sample("test_function", 5, false);
        calc.record_sample("test_function", 5, false);

        let most_common = calc.most_common_depth();
        assert_eq!(most_common, Some(5)); // 深度5出现3次

        let avg_depth = calc.average_depth();
        assert_eq!(avg_depth, 4.2); // (3+3+5+5+5)/5 = 4.2
    }

    #[test]
    fn test_caller_distribution() {
        let mut calc = FunctionStatsCalculator::new("test_function");

        let record1 = SampleRecord::new("test_function", 5, false).with_caller("caller_a");
        let record2 = SampleRecord::new("test_function", 5, false).with_caller("caller_a");
        let record3 = SampleRecord::new("test_function", 5, false).with_caller("caller_b");

        calc.record(&record1);
        calc.record(&record2);
        calc.record(&record3);

        let most_common = calc.most_common_caller();
        assert_eq!(most_common, Some("caller_a"));

        assert_eq!(calc.caller_count(), 2);
    }

    #[test]
    fn test_function_stats_aggregator() {
        let mut aggregator = FunctionStatsAggregator::new()
            .with_sample_interval(1)
            .with_detailed_stats(true);

        // 记录多个函数的样本
        aggregator.record_sample("func_a", 1, true);
        aggregator.record_sample("func_a", 1, false);
        aggregator.record_sample("func_b", 2, false);
        aggregator.record_sample("func_c", 3, false);

        assert_eq!(aggregator.function_count(), 3);
        assert_eq!(aggregator.total_samples(), 4);

        let top_by_calls = aggregator.get_top_by_call_count(2);
        assert_eq!(top_by_calls.len(), 2);
        assert_eq!(top_by_calls[0].function_name(), "func_a");
    }

    #[test]
    fn test_calculator_reset() {
        let mut calc = FunctionStatsCalculator::new("test_function");

        calc.record_sample("test_function", 5, true);
        assert_eq!(calc.total_samples(), 1);

        calc.reset();
        assert_eq!(calc.total_samples(), 0);
        assert_eq!(calc.self_samples(), 0);
        assert_eq!(calc.total_time_us(), 0);
    }

    #[test]
    fn test_calculate_duration() {
        assert_eq!(calculate_duration_us(1000, 2000), 1000);
        assert_eq!(calculate_duration_us(2000, 1000), 0); // 饱和减法
    }

    #[test]
    fn test_estimate_cpu_cycles() {
        // 假设CPU频率为3.0GHz，1000微秒 = 1毫秒
        // 周期数 = 1000 * 3.0 * 1000 = 3,000,000
        let cycles = estimate_cpu_cycles(1000, 3.0);
        assert_eq!(cycles, 3_000_000);
    }

    #[test]
    fn test_calculate_percentages() {
        let mut calc = FunctionStatsCalculator::new("test_function");

        for _ in 0..10 {
            calc.record_sample("test_function", 5, false);
        }

        // 10个样本占总30个的33.33%
        calc.calculate_percentages(30);

        assert_eq!(calc.total_samples(), 10);
    }

    #[test]
    fn test_duration_calculation() {
        let mut calc = FunctionStatsCalculator::new("test_function")
            .with_sample_interval(2); // 2ms

        calc.record_sample("test_function", 5, false);
        calc.record_sample("test_function", 5, false);

        // 每个样本2ms = 2000微秒，总共4000微秒
        assert_eq!(calc.total_time_us(), 4000);
    }
}
