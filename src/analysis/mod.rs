//! 数据分析与统计模块
//!
//! 提供性能分析数据的处理、统计和可视化功能。
//! 包括堆栈数据统计、线程分析、火焰图生成和结果导出等功能。
//!
//! # 主要组件
//!
//! - [`StatsAggregator`]: 统计聚合器，处理堆栈数据并生成统计信息
//! - [`ThreadAnalyzer`]: 线程分析器，按线程分析调用关系和热点路径
//! - [`FunctionStatsCalculator`]: 函数统计计算器
//! - [`FlameGraphBuilder`]: 火焰图数据生成器
//! - [`AnalysisExporter`]: 分析结果导出 trait
//!
//! # 使用示例
//!
//! ```rust,no_run
//! use profiler::analysis::{StatsAggregator, AggregatorConfig};
//! use profiler::stackwalker::CollectedStack;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // 创建聚合器配置
//! let config = AggregatorConfig::default();
//! let mut aggregator = StatsAggregator::new(config);
//!
//! // 处理堆栈数据
//! // for stack in stacks {
//! //     aggregator.process_stack(&stack)?;
//! // }
//!
//! // 获取线程统计
//! // let thread_stats = aggregator.get_all_thread_stats();
//! # Ok(())
//! # }
//! ```

use crate::error::Result;
use crate::types::{FunctionStats, ProcessStats, ThreadStats, ThreadId, ProcessId};

// 子模块定义
mod aggregator;
mod exporter;
mod flamegraph;
mod function_stats;
mod thread_analyzer;

// 公开导出
pub use aggregator::{AggregatorConfig, StatsAggregator};
pub use exporter::{AnalysisExporter, AnalysisResult, CsvExporter, JsonExporter};
pub use flamegraph::{FlameGraph, FlameGraphBuilder, FlameNode};
pub use function_stats::{FunctionStatsCalculator, SampleRecord};
pub use thread_analyzer::{CallGraph, CallNode, CallPath, ThreadAnalyzer, ThreadStatsSummary};

/// 分析器 Trait
///
/// 定义数据分析的标准接口，实现者可以提供不同的分析策略。
///
/// # 线程安全
///
/// 该 trait 要求实现类型是 `Send + Sync` 的，以支持多线程分析。
pub trait Analyzer: Send + Sync {
    /// 处理单个采样堆栈
    ///
    /// # 参数
    /// - `stack`: 收集到的堆栈数据
    ///
    /// # 返回
    /// 处理成功返回 Ok(())，失败返回错误
    fn analyze(&mut self, stack: &crate::stackwalker::CollectedStack) -> Result<()>;

    /// 获取分析结果
    fn get_result(&self) -> AnalysisResult;

    /// 重置分析器状态
    fn reset(&mut self);

    /// 获取已处理的样本数量
    fn sample_count(&self) -> u64;
}

/// 统计聚合器 Trait
///
/// 定义统计信息聚合的标准接口。
pub trait StatsAggregatorTrait: Send + Sync {
    /// 处理堆栈数据
    ///
    /// # 参数
    /// - `stack`: 收集到的堆栈数据
    fn process_stack(&mut self, stack: &crate::stackwalker::CollectedStack) -> Result<()>;

    /// 获取指定线程的统计信息
    fn get_thread_stats(&self, thread_id: ThreadId) -> Option<&ThreadStats>;

    /// 获取所有线程的统计信息
    fn get_all_thread_stats(&self) -> &std::collections::HashMap<ThreadId, ThreadStats>;

    /// 获取指定线程中指定函数的统计信息
    fn get_function_stats(
        &self,
        thread_id: ThreadId,
        function_name: &str,
    ) -> Option<&FunctionStats>;

    /// 获取指定线程中耗时最高的函数列表
    fn get_top_functions(&self, thread_id: ThreadId, limit: usize) -> Vec<&FunctionStats>;

    /// 重置所有统计信息
    fn reset(&mut self);

    /// 获取总样本数
    fn total_samples(&self) -> u64;
}

/// 热点路径分析器 Trait
///
/// 定义热点调用路径识别的标准接口。
pub trait HotPathAnalyzer: Send + Sync {
    /// 识别热点路径
    ///
    /// # 参数
    /// - `depth`: 路径深度限制
    ///
    /// # 返回
    /// 热点路径列表，按热度排序
    fn find_hot_paths(&self, depth: usize) -> Vec<CallPath>;

    /// 获取指定函数的调用者
    fn get_callers(&self, function_name: &str) -> Vec<String>;

    /// 获取指定函数的调用目标
    fn get_callees(&self, function_name: &str) -> Vec<String>;
}

/// 合并多个函数统计信息
///
/// # 参数
/// - `stats`: 函数统计信息列表
///
/// # 返回
/// 合并后的函数统计信息
pub fn merge_function_stats(stats: &[FunctionStats]) -> Option<FunctionStats> {
    if stats.is_empty() {
        return None;
    }

    let mut merged = stats[0].clone();
    for stat in &stats[1..] {
        merged.merge(stat);
    }
    Some(merged)
}

/// 计算百分比
///
/// # 参数
/// - `value`: 当前值
/// - `total`: 总值
///
/// # 返回
/// 百分比值（0-100）
pub fn calculate_percentage(value: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (value as f64 / total as f64) * 100.0
    }
}

/// 格式化时长（微秒到人类可读格式）
///
/// # 参数
/// - `microseconds`: 微秒数
///
/// # 返回
/// 格式化后的字符串
pub fn format_duration(microseconds: u64) -> String {
    if microseconds < 1000 {
        format!("{} μs", microseconds)
    } else if microseconds < 1_000_000 {
        format!("{:.2} ms", microseconds as f64 / 1000.0)
    } else if microseconds < 60_000_000 {
        format!("{:.2} s", microseconds as f64 / 1_000_000.0)
    } else {
        let seconds = microseconds / 1_000_000;
        let minutes = seconds / 60;
        let remaining_secs = seconds % 60;
        format!("{}m {}s", minutes, remaining_secs)
    }
}

/// 分析模块配置
#[derive(Debug, Clone)]
pub struct AnalysisConfig {
    /// 是否启用调用图构建
    pub enable_call_graph: bool,
    /// 是否启用火焰图生成
    pub enable_flame_graph: bool,
    /// 热点路径分析深度
    pub hot_path_depth: usize,
    /// 最小样本数阈值
    pub min_sample_threshold: u64,
    /// 采样间隔（毫秒）
    pub sample_interval_ms: u32,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            enable_call_graph: true,
            enable_flame_graph: true,
            hot_path_depth: 10,
            min_sample_threshold: 1,
            sample_interval_ms: 1,
        }
    }
}

/// 分析统计信息
#[derive(Debug, Clone, Default)]
pub struct AnalysisStats {
    /// 处理的样本总数
    pub total_samples: u64,
    /// 跳过的样本数
    pub skipped_samples: u64,
    /// 成功解析的堆栈数
    pub resolved_stacks: u64,
    /// 失败的堆栈数
    pub failed_stacks: u64,
    /// 分析耗时（微秒）
    pub analysis_duration_us: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_percentage() {
        assert_eq!(calculate_percentage(50, 100), 50.0);
        assert_eq!(calculate_percentage(0, 100), 0.0);
        assert_eq!(calculate_percentage(100, 0), 0.0);
        assert_eq!(calculate_percentage(33, 100), 33.0);
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(500), "500 μs");
        assert_eq!(format_duration(1500), "1.50 ms");
        assert_eq!(format_duration(1_500_000), "1.50 s");
        assert_eq!(format_duration(65_000_000), "1m 5s");
    }

    #[test]
    fn test_analysis_config_default() {
        let config = AnalysisConfig::default();
        assert!(config.enable_call_graph);
        assert!(config.enable_flame_graph);
        assert_eq!(config.hot_path_depth, 10);
        assert_eq!(config.min_sample_threshold, 1);
        assert_eq!(config.sample_interval_ms, 1);
    }
}
