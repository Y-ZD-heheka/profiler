//! 火焰图数据生成模块
//!
//! 提供火焰图格式数据的生成功能，支持folded格式导出。

use crate::error::Result;
use crate::stackwalker::{CollectedStack, ResolvedFrame};
use crate::types::StackFrame;

use std::collections::HashMap;
use tracing::{debug, trace, warn};

/// 火焰图构建器
#[derive(Debug)]
pub struct FlameGraphBuilder {
    /// 根节点
    root: FlameNode,
    /// 总样本数
    total_samples: u64,
    /// 是否反转堆栈（从根到叶）
    reverse_stack: bool,
    /// 最小样本阈值
    min_samples: u64,
    /// 分隔符
    delimiter: String,
}

impl Default for FlameGraphBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl FlameGraphBuilder {
    /// 创建新的火焰图构建器
    pub fn new() -> Self {
        Self {
            root: FlameNode::new("root"),
            total_samples: 0,
            reverse_stack: false,
            min_samples: 1,
            delimiter: ";".to_string(),
        }
    }

    /// 创建带配置的火焰图构建器
    pub fn with_options(reverse_stack: bool, min_samples: u64) -> Self {
        Self {
            root: FlameNode::new("root"),
            total_samples: 0,
            reverse_stack,
            min_samples,
            delimiter: ";".to_string(),
        }
    }

    /// 设置是否反转堆栈
    pub fn with_reverse_stack(mut self, reverse: bool) -> Self {
        self.reverse_stack = reverse;
        self
    }

    /// 设置最小样本阈值
    pub fn with_min_samples(mut self, min_samples: u64) -> Self {
        self.min_samples = min_samples;
        self
    }

    /// 设置分隔符
    pub fn with_delimiter(mut self, delimiter: impl Into<String>) -> Self {
        self.delimiter = delimiter.into();
        self
    }

    /// 添加堆栈
    pub fn add_stack(&mut self, stack: &CollectedStack) {
        if stack.frames.is_empty() {
            trace!("Skipping empty stack");
            return;
        }

        // 构建函数名称列表
        let mut function_names: Vec<String> = stack
            .frames
            .iter()
            .map(|f| f.display_name())
            .collect();

        // 如果需要反转
        if self.reverse_stack {
            function_names.reverse();
        }

        // 插入到火焰图树中
        self.root.insert(&function_names, 1);
        self.total_samples += 1;

        trace!(
            "Added stack with {} frames to flame graph",
            function_names.len()
        );
    }

    /// 批量添加堆栈
    pub fn add_stacks(&mut self, stacks: &[CollectedStack]) {
        for stack in stacks {
            self.add_stack(stack);
        }
        debug!("Added {} stacks to flame graph", stacks.len());
    }

    /// 构建火焰图
    pub fn build(&self) -> FlameGraph {
        let filtered_root = if self.min_samples > 1 {
            self.root.filter_min_samples(self.min_samples)
        } else {
            self.root.clone()
        };

        FlameGraph {
            root: filtered_root,
            total_samples: self.total_samples,
            reverse_stack: self.reverse_stack,
        }
    }

    /// 导出为folded格式
    pub fn export_folded(&self) -> String {
        let mut lines = Vec::new();
        self.build_folded_lines(&self.root, String::new(), &mut lines);

        lines.sort();
        lines.join("\n")
    }

    /// 递归构建folded格式的行
    fn build_folded_lines(&self, node: &FlameNode, prefix: String, lines: &mut Vec<String>) {
        if node.value > 0 && !prefix.is_empty() {
            let path = if prefix.starts_with(&self.delimiter) {
                &prefix[self.delimiter.len()..]
            } else {
                &prefix
            };
            lines.push(format!("{} {}", path, node.value));
        }

        for (name, child) in &node.children {
            let new_prefix = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{}{}{}", prefix, self.delimiter, name)
            };
            self.build_folded_lines(child, new_prefix, lines);
        }
    }

    /// 导出为JSON格式
    pub fn export_json(&self) -> String {
        let flame_graph = self.build();
        serde_json::to_string_pretty(&flame_graph).unwrap_or_default()
    }

    /// 获取总样本数
    pub fn total_samples(&self) -> u64 {
        self.total_samples
    }

    /// 获取根节点
    pub fn root(&self) -> &FlameNode {
        &self.root
    }

    /// 重置构建器
    pub fn reset(&mut self) {
        self.root = FlameNode::new("root");
        self.total_samples = 0;
        debug!("FlameGraphBuilder reset completed");
    }
}

/// 火焰图
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FlameGraph {
    /// 根节点
    pub root: FlameNode,
    /// 总样本数
    pub total_samples: u64,
    /// 是否反转堆栈
    pub reverse_stack: bool,
}

impl FlameGraph {
    /// 创建空的火焰图
    pub fn new() -> Self {
        Self {
            root: FlameNode::new("root"),
            total_samples: 0,
            reverse_stack: false,
        }
    }

    /// 从folded格式字符串解析火焰图
    pub fn from_folded(folded_data: &str) -> Result<Self> {
        let mut root = FlameNode::new("root");
        let mut total_samples = 0u64;

        for line in folded_data.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.rsplitn(2, ' ').collect();
            if parts.len() != 2 {
                warn!("Invalid folded format line: {}", line);
                continue;
            }

            let count: u64 = parts[0].parse().unwrap_or(0);
            let path = parts[1];

            if count > 0 {
                let function_names: Vec<&str> = path.split(';').collect();
                let names: Vec<String> = function_names.iter().map(|s| s.to_string()).collect();
                root.insert(&names, count);
                total_samples += count;
            }
        }

        Ok(Self {
            root,
            total_samples,
            reverse_stack: false,
        })
    }

    /// 获取指定路径的节点
    pub fn get_node(&self, path: &[String]) -> Option<&FlameNode> {
        self.root.get_child(path)
    }

    /// 获取节点的值
    pub fn get_value(&self, path: &[String]) -> u64 {
        self.get_node(path).map(|n| n.value).unwrap_or(0)
    }

    /// 计算节点的百分比
    pub fn get_percentage(&self, path: &[String]) -> f64 {
        if self.total_samples == 0 {
            return 0.0;
        }
        let value = self.get_value(path) as f64;
        (value / self.total_samples as f64) * 100.0
    }

    /// 获取所有叶节点
    pub fn get_leaf_nodes(&self) -> Vec<(Vec<String>, u64)> {
        let mut leaves = Vec::new();
        self.collect_leaves(&self.root, Vec::new(), &mut leaves);
        leaves
    }

    fn collect_leaves(
        &self,
        node: &FlameNode,
        path: Vec<String>,
        leaves: &mut Vec<(Vec<String>, u64)>,
    ) {
        if node.children.is_empty() {
            if node.value > 0 {
                leaves.push((path, node.value));
            }
        } else {
            for (name, child) in &node.children {
                let mut new_path = path.clone();
                new_path.push(name.clone());
                self.collect_leaves(child, new_path, leaves);
            }
        }
    }

    /// 获取最热的调用路径
    pub fn get_hottest_paths(&self, limit: usize) -> Vec<(Vec<String>, u64)> {
        let mut leaves = self.get_leaf_nodes();
        leaves.sort_by(|a, b| b.1.cmp(&a.1));
        leaves.into_iter().take(limit).collect()
    }

    /// 合并另一个火焰图
    pub fn merge(&mut self, other: &FlameGraph) {
        self.root.merge(&other.root);
        self.total_samples += other.total_samples;
    }

    /// 过滤低样本节点
    pub fn filter_min_samples(&mut self, min_samples: u64) {
        self.root = self.root.filter_min_samples(min_samples);
    }

    /// 获取节点数量
    pub fn node_count(&self) -> usize {
        self.root.count_nodes()
    }

    /// 获取树深度
    pub fn depth(&self) -> usize {
        self.root.depth()
    }

    /// 导出为folded格式
    pub fn to_folded_format(&self) -> String {
        let mut lines = Vec::new();
        self.build_folded_lines(&self.root, String::new(), &mut lines, ";");
        lines.sort();
        lines.join("\n")
    }

    fn build_folded_lines(&self, node: &FlameNode, prefix: String, lines: &mut Vec<String>, delimiter: &str) {
        if node.value > 0 && !prefix.is_empty() {
            let path = if prefix.starts_with(delimiter) {
                &prefix[delimiter.len()..]
            } else {
                &prefix
            };
            lines.push(format!("{} {}", path, node.value));
        }

        for (name, child) in &node.children {
            let new_prefix = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{}{}{}", prefix, delimiter, name)
            };
            self.build_folded_lines(child, new_prefix, lines, delimiter);
        }
    }
}

impl Default for FlameGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// 火焰图节点
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct FlameNode {
    /// 节点名称
    pub name: String,
    /// 样本数
    pub value: u64,
    /// 子节点
    pub children: HashMap<String, FlameNode>,
}

impl FlameNode {
    /// 创建新的火焰图节点
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: 0,
            children: HashMap::new(),
        }
    }

    /// 创建带值的节点
    pub fn with_value(name: impl Into<String>, value: u64) -> Self {
        Self {
            name: name.into(),
            value,
            children: HashMap::new(),
        }
    }

    /// 插入路径
    pub fn insert(&mut self, path: &[String], value: u64) {
        if path.is_empty() {
            self.value += value;
            return;
        }

        let name = &path[0];
        let child = self
            .children
            .entry(name.clone())
            .or_insert_with(|| FlameNode::new(name.clone()));

        child.insert(&path[1..], value);
        self.value += value;
    }

    /// 获取子节点
    pub fn get_child(&self, path: &[String]) -> Option<&FlameNode> {
        if path.is_empty() {
            return Some(self);
        }

        self.children.get(&path[0])?.get_child(&path[1..])
    }

    /// 获取可变的子节点
    pub fn get_child_mut(&mut self, path: &[String]) -> Option<&mut FlameNode> {
        if path.is_empty() {
            return Some(self);
        }

        self.children.get_mut(&path[0])?.get_child_mut(&path[1..])
    }

    /// 合并另一个节点
    pub fn merge(&mut self, other: &FlameNode) {
        self.value += other.value;

        for (name, other_child) in &other.children {
            let child = self
                .children
                .entry(name.clone())
                .or_insert_with(|| FlameNode::new(name.clone()));
            child.merge(other_child);
        }
    }

    /// 过滤低样本节点
    pub fn filter_min_samples(&self, min_samples: u64) -> Self {
        if self.value < min_samples {
            return FlameNode::new(&self.name);
        }

        let mut filtered = FlameNode::with_value(&self.name, self.value);

        for (name, child) in &self.children {
            let filtered_child = child.filter_min_samples(min_samples);
            if filtered_child.value >= min_samples || !filtered_child.children.is_empty() {
                filtered.children.insert(name.clone(), filtered_child);
            }
        }

        filtered
    }

    /// 计算节点数量
    pub fn count_nodes(&self) -> usize {
        1 + self
            .children
            .values()
            .map(|c| c.count_nodes())
            .sum::<usize>()
    }

    /// 计算树深度
    pub fn depth(&self) -> usize {
        if self.children.is_empty() {
            1
        } else {
            1 + self
                .children
                .values()
                .map(|c| c.depth())
                .max()
                .unwrap_or(0)
        }
    }

    /// 获取子节点值的总和
    pub fn children_value_sum(&self) -> u64 {
        self.children.values().map(|c| c.value).sum()
    }

    /// 是否是叶节点
    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }
}

/// 从CallStack构建火焰图路径
pub fn build_flame_path(stack: &[StackFrame], reverse: bool) -> Vec<String> {
    let mut path: Vec<String> = stack.iter().map(|f| f.full_function_name()).collect();

    if reverse {
        path.reverse();
    }

    path
}

/// 合并多个火焰图
pub fn merge_flame_graphs(graphs: &[FlameGraph]) -> FlameGraph {
    if graphs.is_empty() {
        return FlameGraph::new();
    }

    let mut merged = graphs[0].clone();
    for graph in &graphs[1..] {
        merged.merge(graph);
    }

    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stackwalker::FrameType;
    use crate::types::ModuleInfo;

    fn create_test_stack(frames: Vec<&str>) -> CollectedStack {
        let resolved_frames: Vec<ResolvedFrame> = frames
            .into_iter()
            .map(|module| {
                ResolvedFrame::with_type(0x00401000, FrameType::User)
                    .with_module(ModuleInfo::new(0x00400000, 0x10000, module))
            })
            .collect();

        CollectedStack::new(1000, 1, 1000, resolved_frames)
    }

    #[test]
    fn test_flame_node_insert() {
        let mut root = FlameNode::new("root");

        root.insert(&["a".to_string(), "b".to_string(), "c".to_string()], 10);

        assert_eq!(root.value, 10);
        assert!(root.children.contains_key("a"));

        let node_a = root.children.get("a").unwrap();
        assert_eq!(node_a.value, 10);
        assert!(node_a.children.contains_key("b"));
    }

    #[test]
    fn test_flame_node_merge() {
        let mut node1 = FlameNode::new("root");
        node1.insert(&["a".to_string(), "b".to_string()], 10);

        let mut node2 = FlameNode::new("root");
        node2.insert(&["a".to_string(), "c".to_string()], 5);

        node1.merge(&node2);

        assert_eq!(node1.value, 15);
        assert!(node1.children.contains_key("a"));

        let node_a = node1.children.get("a").unwrap();
        assert_eq!(node_a.value, 15);
        assert!(node_a.children.contains_key("b"));
        assert!(node_a.children.contains_key("c"));
    }

    #[test]
    fn test_flame_graph_builder() {
        let mut builder = FlameGraphBuilder::new();

        let stack1 = create_test_stack(vec!["app.exe", "lib.dll", "kernel32.dll"]);
        let stack2 = create_test_stack(vec!["app.exe", "lib.dll", "ntdll.dll"]);

        builder.add_stack(&stack1);
        builder.add_stack(&stack2);

        let flame_graph = builder.build();

        assert_eq!(flame_graph.total_samples, 2);
        assert!(flame_graph.root.value >= 2);
    }

    #[test]
    fn test_export_folded() {
        let mut builder = FlameGraphBuilder::new();

        let stack = create_test_stack(vec!["app.exe", "lib.dll", "kernel32.dll"]);
        builder.add_stack(&stack);

        let folded = builder.export_folded();
        assert!(!folded.is_empty());
    }

    #[test]
    fn test_flame_node_filter() {
        let mut root = FlameNode::new("root");

        root.insert(&["a".to_string()], 100);
        root.insert(&["b".to_string()], 5);
        root.insert(&["c".to_string()], 3);

        let filtered = root.filter_min_samples(10);

        assert!(filtered.children.contains_key("a"));
        assert!(!filtered.children.contains_key("b"));
        assert!(!filtered.children.contains_key("c"));
    }

    #[test]
    fn test_from_folded() {
        let folded_data = "a;b;c 10\na;b;d 5\na;e 3";

        let flame_graph = FlameGraph::from_folded(folded_data).unwrap();

        assert_eq!(flame_graph.total_samples, 18);
        assert!(flame_graph.root.children.contains_key("a"));

        let node_a = flame_graph.root.children.get("a").unwrap();
        assert_eq!(node_a.value, 18);
    }

    #[test]
    fn test_flame_node_count() {
        let mut root = FlameNode::new("root");

        root.insert(&["a".to_string(), "b".to_string(), "c".to_string()], 10);
        root.insert(&["a".to_string(), "d".to_string()], 5);

        assert_eq!(root.count_nodes(), 5);
    }

    #[test]
    fn test_flame_node_depth() {
        let mut root = FlameNode::new("root");

        root.insert(&["a".to_string(), "b".to_string(), "c".to_string()], 10);
        root.insert(&["a".to_string(), "d".to_string()], 5);

        assert_eq!(root.depth(), 4);
    }

    #[test]
    fn test_get_hottest_paths() {
        let mut builder = FlameGraphBuilder::new();

        for _ in 0..10 {
            let stack = create_test_stack(vec!["app.exe", "hot.dll"]);
            builder.add_stack(&stack);
        }

        for _ in 0..5 {
            let stack = create_test_stack(vec!["app.exe", "cold.dll"]);
            builder.add_stack(&stack);
        }

        let flame_graph = builder.build();
        let hottest = flame_graph.get_hottest_paths(2);

        assert!(!hottest.is_empty());
        assert_eq!(hottest[0].1, 10);
    }

    #[test]
    fn test_build_flame_path() {
        let frames = vec![
            StackFrame::with_symbol(0x001, "app.exe", "main"),
            StackFrame::with_symbol(0x002, "app.exe", "run"),
            StackFrame::with_symbol(0x003, "lib.dll", "process"),
        ];

        let path = build_flame_path(&frames, false);
        assert_eq!(path.len(), 3);
        assert_eq!(path[0], "app.exe!main");

        let reversed = build_flame_path(&frames, true);
        assert_eq!(reversed[0], "lib.dll!process");
    }

    #[test]
    fn test_merge_flame_graphs() {
        let mut builder1 = FlameGraphBuilder::new();
        let stack1 = create_test_stack(vec!["app.exe", "func_a.dll"]);
        builder1.add_stack(&stack1);

        let mut builder2 = FlameGraphBuilder::new();
        let stack2 = create_test_stack(vec!["app.exe", "func_b.dll"]);
        builder2.add_stack(&stack2);

        let merged = merge_flame_graphs(&[builder1.build(), builder2.build()]);

        assert_eq!(merged.total_samples, 2);
    }

    #[test]
    fn test_flame_graph_builder_reset() {
        let mut builder = FlameGraphBuilder::new();

        let stack = create_test_stack(vec!["app.exe"]);
        builder.add_stack(&stack);

        assert_eq!(builder.total_samples(), 1);

        builder.reset();

        assert_eq!(builder.total_samples(), 0);
        assert_eq!(builder.root().value, 0);
    }
}
