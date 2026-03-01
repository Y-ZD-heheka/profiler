//! 线程分析器模块
//!
//! 提供按线程的分析逻辑，实现调用关系图构建和热点路径识别。

use crate::error::{ProfilerError, Result};
use crate::stackwalker::{CollectedStack, ResolvedFrame};
use crate::types::{FunctionStats, ProcessId, ThreadId, Timestamp};

use std::collections::HashMap;
use tracing::{debug, trace, warn};

/// 线程分析器
///
/// 分析单个线程的调用堆栈数据，构建调用关系图并识别热点路径。
#[derive(Debug)]
pub struct ThreadAnalyzer {
    /// 线程 ID
    pub thread_id: ThreadId,
    /// 所属进程 ID
    pub process_id: ProcessId,
    /// 函数统计映射（函数名 -> 统计信息）
    pub function_stats: HashMap<String, FunctionStats>,
    /// 调用关系图
    pub call_graph: CallGraph,
    /// 总样本数
    total_samples: u64,
    /// 首次采样时间
    first_sample_time: Option<Timestamp>,
    /// 最后采样时间
    last_sample_time: Option<Timestamp>,
    /// 调用栈历史（用于路径分析）
    call_stack_history: Vec<Vec<String>>,
    /// 配置选项
    config: ThreadAnalyzerConfig,
}

/// 线程分析器配置
#[derive(Debug, Clone)]
pub struct ThreadAnalyzerConfig {
    /// 最大调用栈历史记录数
    pub max_history_size: usize,
    /// 是否跟踪调用来源
    pub track_call_sources: bool,
    /// 热点路径最小样本数
    pub hot_path_threshold: u64,
}

impl Default for ThreadAnalyzerConfig {
    fn default() -> Self {
        Self {
            max_history_size: 10000,
            track_call_sources: true,
            hot_path_threshold: 1,
        }
    }
}

/// 调用关系图
///
/// 记录函数间的调用关系，支持热点路径识别。
#[derive(Debug, Clone, Default)]
pub struct CallGraph {
    /// 调用节点映射（函数名 -> 节点）
    pub nodes: HashMap<String, CallNode>,
    /// 调用边（调用者 -> 被调用者 -> 次数）
    pub edges: HashMap<String, HashMap<String, u64>>,
    /// 反向边（被调用者 -> 调用者 -> 次数）
    pub reverse_edges: HashMap<String, HashMap<String, u64>>,
}

/// 调用节点
///
/// 表示调用图中的一个函数节点。
#[derive(Debug, Clone)]
pub struct CallNode {
    /// 函数名称
    pub function_name: String,
    /// 模块名称
    pub module_name: Option<String>,
    /// 被调用次数
    pub call_count: u64,
    /// 作为叶节点的次数（出现在栈顶）
    pub leaf_count: u64,
    /// 平均调用深度
    pub avg_call_depth: f64,
    /// 总调用深度（用于计算平均值）
    total_depth: u64,
}

impl CallNode {
    /// 创建新的调用节点
    pub fn new(function_name: impl Into<String>) -> Self {
        Self {
            function_name: function_name.into(),
            module_name: None,
            call_count: 0,
            leaf_count: 0,
            avg_call_depth: 0.0,
            total_depth: 0,
        }
    }

    /// 记录一次调用
    pub fn record_call(&mut self, depth: usize, is_leaf: bool) {
        self.call_count += 1;
        self.total_depth += depth as u64;
        self.avg_call_depth = self.total_depth as f64 / self.call_count as f64;

        if is_leaf {
            self.leaf_count += 1;
        }
    }

    /// 设置模块名称
    pub fn with_module(mut self, module: impl Into<String>) -> Self {
        self.module_name = Some(module.into());
        self
    }
}

/// 调用路径
///
/// 表示一条完整的调用链。
#[derive(Debug, Clone)]
pub struct CallPath {
    /// 路径 ID
    pub id: u64,
    /// 函数调用链（从根到叶）
    pub functions: Vec<String>,
    /// 出现次数
    pub count: u64,
    /// 总耗时（微秒）
    pub total_time_us: u64,
    /// 深度
    pub depth: usize,
}

impl CallPath {
    /// 创建新的调用路径
    pub fn new(id: u64, functions: Vec<String>) -> Self {
        let depth = functions.len();
        Self {
            id,
            functions,
            count: 0,
            total_time_us: 0,
            depth,
        }
    }

    /// 记录一次出现
    pub fn record(&mut self, sample_interval_us: u64) {
        self.count += 1;
        self.total_time_us += sample_interval_us;
    }

    /// 获取路径字符串表示
    pub fn path_string(&self) -> String {
        self.functions.join(" -> ")
    }

    /// 计算平均耗时
    pub fn average_time_us(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.total_time_us as f64 / self.count as f64
        }
    }
}

/// 线程统计摘要
#[derive(Debug, Clone)]
pub struct ThreadStatsSummary {
    /// 线程 ID
    pub thread_id: ThreadId,
    /// 进程 ID
    pub process_id: ProcessId,
    /// 总样本数
    pub total_samples: u64,
    /// 唯一函数数量
    pub unique_functions: usize,
    /// 平均调用深度
    pub avg_call_depth: f64,
    /// 最大调用深度
    pub max_call_depth: usize,
    /// 最耗时的函数
    pub hottest_function: Option<String>,
    /// 线程总耗时（微秒）
    pub total_time_us: u64,
}

impl ThreadAnalyzer {
    /// 创建新的线程分析器
    pub fn new(thread_id: ThreadId, process_id: ProcessId) -> Self {
        Self {
            thread_id,
            process_id,
            function_stats: HashMap::new(),
            call_graph: CallGraph::default(),
            total_samples: 0,
            first_sample_time: None,
            last_sample_time: None,
            call_stack_history: Vec::new(),
            config: ThreadAnalyzerConfig::default(),
        }
    }

    /// 创建带配置的线程分析器
    pub fn with_config(
        thread_id: ThreadId,
        process_id: ProcessId,
        config: ThreadAnalyzerConfig,
    ) -> Self {
        Self {
            thread_id,
            process_id,
            function_stats: HashMap::new(),
            call_graph: CallGraph::default(),
            total_samples: 0,
            first_sample_time: None,
            last_sample_time: None,
            call_stack_history: Vec::with_capacity(config.max_history_size),
            config,
        }
    }

    /// 记录堆栈
    pub fn record_stack(&mut self, stack: &CollectedStack) -> Result<()> {
        // 验证线程ID匹配
        if stack.thread_id != self.thread_id {
            return Err(ProfilerError::Generic(format!(
                "Thread ID mismatch: expected {}, got {}",
                self.thread_id, stack.thread_id
            )));
        }

        trace!("Recording stack for thread {}", self.thread_id);

        // 更新时间戳
        if self.first_sample_time.is_none() {
            self.first_sample_time = Some(stack.timestamp);
        }
        self.last_sample_time = Some(stack.timestamp);

        // 提取函数名称列表（从栈顶到栈底）
        let function_names: Vec<String> = stack
            .frames
            .iter()
            .map(|f| f.display_name())
            .collect();

        if function_names.is_empty() {
            warn!("Empty function names in stack for thread {}", self.thread_id);
            return Ok(());
        }

        // 记录调用栈历史
        if self.call_stack_history.len() < self.config.max_history_size {
            self.call_stack_history.push(function_names.clone());
        }

        // 更新调用图
        self.update_call_graph(&function_names);

        // 更新函数统计
        self.update_function_stats(stack, &function_names);

        self.total_samples += 1;

        debug!(
            "Recorded stack with {} frames for thread {}",
            function_names.len(),
            self.thread_id
        );

        Ok(())
    }

    /// 更新调用图
    fn update_call_graph(&mut self, function_names: &[String]) {
        let depth = function_names.len();

        // 更新节点统计
        for (i, func_name) in function_names.iter().enumerate() {
            let is_leaf = i == 0; // 栈顶是叶节点
            let call_depth = depth - i - 1; // 调用深度（从0开始）

            let node = self
                .call_graph
                .nodes
                .entry(func_name.clone())
                .or_insert_with(|| CallNode::new(func_name.clone()));

            node.record_call(call_depth, is_leaf);
        }

        // 更新调用边（从栈底到栈顶方向）
        for i in (1..function_names.len()).rev() {
            let caller = &function_names[i];
            let callee = &function_names[i - 1];

            // 正向边：caller -> callee
            self.call_graph
                .edges
                .entry(caller.clone())
                .or_insert_with(HashMap::new)
                .entry(callee.clone())
                .and_modify(|count| *count += 1)
                .or_insert(1);

            // 反向边：callee -> caller
            self.call_graph
                .reverse_edges
                .entry(callee.clone())
                .or_insert_with(HashMap::new)
                .entry(caller.clone())
                .and_modify(|count| *count += 1)
                .or_insert(1);
        }
    }

    /// 更新函数统计
    fn update_function_stats(&mut self, stack: &CollectedStack, function_names: &[String]) {
        let sample_interval_us = 1000; // 默认1ms = 1000μs

        for (i, func_name) in function_names.iter().enumerate() {
            let is_leaf = i == 0;
            let frame = &stack.frames[i];

            let stats = self
                .function_stats
                .entry(func_name.clone())
                .or_insert_with(|| {
                    let mut s = FunctionStats::new(func_name.clone());
                    s.thread_id = Some(self.thread_id);
                    if let Some(ref module) = frame.module {
                        s.module_name = Some(module.name.clone());
                    }
                    s
                });

            // 更新统计
            stats.total_time_us += sample_interval_us;

            if is_leaf {
                stats.self_time_us += sample_interval_us;
                stats.call_count += 1;
            }

            // 更新耗时范围
            stats.min_time_us = stats.min_time_us.min(sample_interval_us);
            if stats.max_time_us == 0 {
                stats.max_time_us = sample_interval_us;
            } else {
                stats.max_time_us = stats.max_time_us.max(sample_interval_us);
            }
        }
    }

    /// 计算函数自身耗时（排除子调用）
    pub fn calculate_exclusive_time(&mut self) {
        // 构建每个函数的直接子函数集合
        let mut child_times: HashMap<String, u64> = HashMap::new();

        for (caller, callees) in &self.call_graph.edges {
            let total_callee_time: u64 = callees
                .values()
                .filter_map(|count| {
                    self.function_stats
                        .values()
                        .find(|s| s.call_count == *count)
                        .map(|s| s.total_time_us)
                })
                .sum();

            child_times.insert(caller.clone(), total_callee_time);
        }

        // 更新自身耗时
        for (func_name, stats) in &mut self.function_stats {
            if let Some(child_time) = child_times.get(func_name) {
                stats.self_time_us = stats.total_time_us.saturating_sub(*child_time);
            }

            // 更新平均耗时
            if stats.call_count > 0 {
                stats.average_time_us = stats.total_time_us as f64 / stats.call_count as f64;
            }
        }

        debug!(
            "Calculated exclusive times for {} functions in thread {}",
            self.function_stats.len(),
            self.thread_id
        );
    }

    /// 获取热点调用路径
    pub fn get_hot_paths(&self, depth: usize) -> Vec<CallPath> {
        // 从调用栈历史中提取路径
        let mut path_counts: HashMap<String, (Vec<String>, u64)> = HashMap::new();

        for stack in &self.call_stack_history {
            // 限制路径深度
            let path: Vec<String> = stack.iter().take(depth).cloned().collect();
            let path_key = path.join("|");

            path_counts
                .entry(path_key)
                .and_modify(|(_, count)| *count += 1)
                .or_insert((path, 1));
        }

        // 转换为 CallPath 列表并排序
        let mut paths: Vec<CallPath> = path_counts
            .into_iter()
            .enumerate()
            .map(|(id, (_, (path_vec, count)))| {
                let mut call_path = CallPath::new(id as u64, path_vec);
                call_path.count = count;
                call_path.total_time_us = count * 1000; // 假设每次采样1ms
                call_path
            })
            .filter(|p| p.count >= self.config.hot_path_threshold)
            .collect();

        paths.sort_by(|a, b| b.count.cmp(&a.count));

        paths
    }

    /// 获取指定函数的调用者
    pub fn get_callers(&self, function_name: &str) -> Vec<(String, u64)> {
        self.call_graph
            .reverse_edges
            .get(function_name)
            .map(|callers| {
                let mut result: Vec<(String, u64)> = callers
                    .iter()
                    .map(|(k, v)| (k.clone(), *v))
                    .collect();
                result.sort_by(|a, b| b.1.cmp(&a.1));
                result
            })
            .unwrap_or_default()
    }

    /// 获取指定函数的调用目标
    pub fn get_callees(&self, function_name: &str) -> Vec<(String, u64)> {
        self.call_graph
            .edges
            .get(function_name)
            .map(|callees| {
                let mut result: Vec<(String, u64)> = callees
                    .iter()
                    .map(|(k, v)| (k.clone(), *v))
                    .collect();
                result.sort_by(|a, b| b.1.cmp(&a.1));
                result
            })
            .unwrap_or_default()
    }

    /// 获取统计摘要
    pub fn get_stats_summary(&self) -> ThreadStatsSummary {
        let total_time_us = self.total_samples * 1000; // 假设每次采样1ms

        // 计算平均调用深度
        let avg_depth = if !self.call_stack_history.is_empty() {
            let total_depth: usize = self.call_stack_history.iter().map(|s| s.len()).sum();
            total_depth as f64 / self.call_stack_history.len() as f64
        } else {
            0.0
        };

        // 找出最大调用深度
        let max_depth = self
            .call_stack_history
            .iter()
            .map(|s| s.len())
            .max()
            .unwrap_or(0);

        // 找出最耗时的函数
        let hottest_function = self
            .function_stats
            .iter()
            .max_by_key(|(_, stats)| stats.total_time_us)
            .map(|(name, _)| name.clone());

        ThreadStatsSummary {
            thread_id: self.thread_id,
            process_id: self.process_id,
            total_samples: self.total_samples,
            unique_functions: self.function_stats.len(),
            avg_call_depth: avg_depth,
            max_call_depth: max_depth,
            hottest_function,
            total_time_us,
        }
    }

    /// 获取最耗时的函数列表
    pub fn get_hottest_functions(&self, limit: usize) -> Vec<&FunctionStats> {
        let mut functions: Vec<&FunctionStats> = self.function_stats.values().collect();
        functions.sort_by(|a, b| b.total_time_us.cmp(&a.total_time_us));
        functions.into_iter().take(limit).collect()
    }

    /// 获取调用次数最多的函数列表
    pub fn get_most_called_functions(&self, limit: usize) -> Vec<&FunctionStats> {
        let mut functions: Vec<&FunctionStats> = self.function_stats.values().collect();
        functions.sort_by(|a, b| b.call_count.cmp(&a.call_count));
        functions.into_iter().take(limit).collect()
    }

    /// 获取指定函数的统计信息
    pub fn get_function_stats(&self, function_name: &str) -> Option<&FunctionStats> {
        self.function_stats.get(function_name)
    }

    /// 重置分析器状态
    pub fn reset(&mut self) {
        self.function_stats.clear();
        self.call_graph = CallGraph::default();
        self.total_samples = 0;
        self.first_sample_time = None;
        self.last_sample_time = None;
        self.call_stack_history.clear();
        debug!("ThreadAnalyzer reset completed for thread {}", self.thread_id);
    }

    /// 获取总样本数
    pub fn total_samples(&self) -> u64 {
        self.total_samples
    }

    /// 获取采样时间范围
    pub fn sampling_duration_us(&self) -> u64 {
        match (self.first_sample_time, self.last_sample_time) {
            (Some(first), Some(last)) => last.saturating_sub(first),
            _ => 0,
        }
    }
}

impl CallGraph {
    /// 创建空的调用关系图
    pub fn new() -> Self {
        Self::default()
    }

    /// 获取节点的入度（被调用次数）
    pub fn in_degree(&self, function_name: &str) -> u64 {
        self.reverse_edges
            .get(function_name)
            .map(|edges| edges.values().sum())
            .unwrap_or(0)
    }

    /// 获取节点的出度（调用其他函数次数）
    pub fn out_degree(&self, function_name: &str) -> u64 {
        self.edges
            .get(function_name)
            .map(|edges| edges.values().sum())
            .unwrap_or(0)
    }

    /// 获取所有叶节点（没有调用其他函数的节点）
    pub fn get_leaf_nodes(&self) -> Vec<&CallNode> {
        self.nodes
            .values()
            .filter(|node| !self.edges.contains_key(&node.function_name))
            .collect()
    }

    /// 获取所有根节点（没有被其他函数调用的节点）
    pub fn get_root_nodes(&self) -> Vec<&CallNode> {
        self.nodes
            .values()
            .filter(|node| !self.reverse_edges.contains_key(&node.function_name))
            .collect()
    }

    /// 计算图的总边数
    pub fn total_edges(&self) -> u64 {
        self.edges.values().map(|m| m.len() as u64).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stackwalker::FrameType;
    use crate::types::ModuleInfo;

    fn create_test_stack(
        thread_id: ThreadId,
        process_id: ProcessId,
        functions: Vec<(&str, &str)>,
    ) -> CollectedStack {
        let resolved_frames: Vec<ResolvedFrame> = functions
            .into_iter()
            .map(|(module, _func)| {
                ResolvedFrame::with_type(0x00401000, FrameType::User)
                    .with_module(ModuleInfo::new(0x00400000, 0x10000, module))
            })
            .collect();

        CollectedStack::new(process_id, thread_id, 1000, resolved_frames)
    }

    #[test]
    fn test_thread_analyzer_new() {
        let analyzer = ThreadAnalyzer::new(1, 1000);

        assert_eq!(analyzer.thread_id, 1);
        assert_eq!(analyzer.process_id, 1000);
        assert_eq!(analyzer.total_samples(), 0);
    }

    #[test]
    fn test_record_stack() {
        let mut analyzer = ThreadAnalyzer::new(1, 1000);

        let stack = create_test_stack(
            1,
            1000,
            vec![
                ("test.exe", "leaf_function"),
                ("test.exe", "middle_function"),
                ("test.exe", "root_function"),
            ],
        );

        analyzer.record_stack(&stack).unwrap();

        assert_eq!(analyzer.total_samples(), 1);
        assert_eq!(analyzer.function_stats.len(), 3);
        assert_eq!(analyzer.call_graph.nodes.len(), 3);
    }

    #[test]
    fn test_call_graph_building() {
        let mut analyzer = ThreadAnalyzer::new(1, 1000);

        for _ in 0..5 {
            let stack = create_test_stack(
                1,
                1000,
                vec![
                    ("test.exe", "leaf_function"),
                    ("test.exe", "middle_function"),
                    ("test.exe", "root_function"),
                ],
            );
            analyzer.record_stack(&stack).unwrap();
        }

        let callees = analyzer.get_callees("root_function");
        assert!(!callees.is_empty());

        let callers = analyzer.get_callers("test.exe+0x1000");
        assert!(!callers.is_empty());
    }

    #[test]
    fn test_hot_paths() {
        let mut analyzer = ThreadAnalyzer::new(1, 1000);

        for _ in 0..10 {
            let stack = create_test_stack(
                1,
                1000,
                vec![
                    ("test.exe", "leaf_function"),
                    ("test.exe", "middle_function"),
                    ("test.exe", "root_function"),
                ],
            );
            analyzer.record_stack(&stack).unwrap();
        }

        let hot_paths = analyzer.get_hot_paths(10);
        assert!(!hot_paths.is_empty());
        assert_eq!(hot_paths[0].count, 10);
    }

    #[test]
    fn test_stats_summary() {
        let mut analyzer = ThreadAnalyzer::new(1, 1000);

        let stack = create_test_stack(
            1,
            1000,
            vec![
                ("test.exe", "function_a"),
                ("test.exe", "function_b"),
            ],
        );

        analyzer.record_stack(&stack).unwrap();

        let summary = analyzer.get_stats_summary();
        assert_eq!(summary.thread_id, 1);
        assert_eq!(summary.process_id, 1000);
        assert_eq!(summary.total_samples, 1);
        assert_eq!(summary.unique_functions, 2);
    }

    #[test]
    fn test_thread_id_mismatch() {
        let mut analyzer = ThreadAnalyzer::new(1, 1000);

        let stack = create_test_stack(
            2, // 不同的线程ID
            1000,
            vec![("test.exe", "function_a")],
        );

        let result = analyzer.record_stack(&stack);
        assert!(result.is_err());
    }

    #[test]
    fn test_call_node() {
        let mut node = CallNode::new("test_function").with_module("test.exe");

        node.record_call(5, true);
        assert_eq!(node.call_count, 1);
        assert_eq!(node.leaf_count, 1);
        assert_eq!(node.avg_call_depth, 5.0);

        node.record_call(3, false);
        assert_eq!(node.call_count, 2);
        assert_eq!(node.leaf_count, 1);
        assert_eq!(node.avg_call_depth, 4.0);
    }

    #[test]
    fn test_call_path() {
        let mut path = CallPath::new(1, vec!["a".to_string(), "b".to_string(), "c".to_string()]);

        assert_eq!(path.depth, 3);
        assert_eq!(path.path_string(), "a -> b -> c");

        path.record(1000);
        path.record(2000);

        assert_eq!(path.count, 2);
        assert_eq!(path.total_time_us, 3000);
        assert_eq!(path.average_time_us(), 1500.0);
    }
}
