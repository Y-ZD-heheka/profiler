//! 堆栈过滤器实现
//!
//! 提供堆栈过滤功能，用于排除不需要的堆栈或帧。
//! 支持系统模块过滤、深度截断和组合过滤器。

use crate::stackwalker::{CollectedStack, FrameType, ResolvedFrame, is_system_module};
use crate::types::ProcessId;
use std::collections::HashSet;

/// 堆栈过滤器 Trait
///
/// 定义过滤堆栈的标准接口。
///
/// # 示例
///
/// ```rust
/// use profiler::stackwalker::{StackFilter, CollectedStack, FrameType};
///
/// struct MyFilter;
///
/// impl StackFilter for MyFilter {
///     fn should_include(&self, stack: &CollectedStack) -> bool {
///         // 只包含用户态堆栈
///         !stack.is_kernel_stack
///     }
///
///     fn should_include_frame(&self, frame: &ResolvedFrame) -> bool {
///         frame.frame_type == FrameType::User
///     }
///
///     fn name(&self) -> &str {
///         "MyFilter"
///     }
/// }
/// ```
pub trait StackFilter: Send + Sync {
    /// 检查堆栈是否应该被包含
    ///
    /// # 参数
    /// - `stack`: 收集的堆栈
    ///
    /// # 返回
    /// 如果应该包含返回 true，否则返回 false
    fn should_include(&self, stack: &CollectedStack) -> bool;

    /// 检查单个帧是否应该被包含
    ///
    /// # 参数
    /// - `frame`: 解析后的帧
    ///
    /// # 返回
    /// 如果应该包含返回 true，否则返回 false
    fn should_include_frame(&self, frame: &ResolvedFrame) -> bool;

    /// 获取过滤器名称
    fn name(&self) -> &str;

    /// 获取过滤器的描述
    fn description(&self) -> String {
        "".to_string()
    }
}

/// 系统模块过滤器
///
/// 过滤掉系统模块（如 ntdll.dll, kernel32.dll 等）的帧。
pub struct SystemModuleFilter {
    /// 额外要过滤的模块列表
    extra_modules: HashSet<String>,
    /// 是否反转过滤逻辑（只保留系统模块）
    invert: bool,
    /// 是否过滤整个堆栈（如果包含系统模块）
    filter_entire_stack: bool,
}

impl SystemModuleFilter {
    /// 创建新的系统模块过滤器
    pub fn new() -> Self {
        Self {
            extra_modules: HashSet::new(),
            invert: false,
            filter_entire_stack: false,
        }
    }

    /// 创建反转的过滤器（只保留系统模块）
    pub fn inverted() -> Self {
        Self {
            extra_modules: HashSet::new(),
            invert: true,
            filter_entire_stack: false,
        }
    }

    /// 添加额外的模块到过滤列表
    pub fn add_module(&mut self, module: impl Into<String>) -> &mut Self {
        self.extra_modules.insert(module.into().to_lowercase());
        self
    }

    /// 设置是否过滤整个堆栈
    pub fn filter_entire_stack(mut self, enable: bool) -> Self {
        self.filter_entire_stack = enable;
        self
    }

    /// 检查模块是否应该被过滤
    fn is_system_module(&self, module_name: &str) -> bool {
        let is_system = is_system_module(module_name)
            || self.extra_modules.iter().any(|m| module_name.to_lowercase().contains(m));

        if self.invert {
            !is_system
        } else {
            is_system
        }
    }
}

impl Default for SystemModuleFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl StackFilter for SystemModuleFilter {
    fn should_include(&self, stack: &CollectedStack) -> bool {
        if !self.filter_entire_stack {
            return true;
        }

        // 如果堆栈中所有帧都是系统模块，过滤掉
        let has_non_system = stack.frames.iter().any(|f| {
            f.module.as_ref().map(|m| !self.is_system_module(&m.name)).unwrap_or(true)
        });

        if self.invert {
            !has_non_system
        } else {
            has_non_system
        }
    }

    fn should_include_frame(&self, frame: &ResolvedFrame) -> bool {
        match &frame.module {
            Some(module) => !self.is_system_module(&module.name),
            None => true,
        }
    }

    fn name(&self) -> &str {
        "SystemModuleFilter"
    }

    fn description(&self) -> String {
        if self.invert {
            "Filters out non-system modules".to_string()
        } else {
            "Filters out system modules (ntdll.dll, kernel32.dll, etc.)".to_string()
        }
    }
}

/// 深度过滤器
///
/// 基于堆栈深度的过滤和截断。
pub struct DepthFilter {
    /// 最大深度
    max_depth: usize,
    /// 最小深度
    min_depth: usize,
    /// 操作：截断或过滤
    action: DepthAction,
}

/// 深度过滤操作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepthAction {
    /// 截断到最大深度
    Truncate,
    /// 过滤掉不符合深度要求的堆栈
    Filter,
}

impl DepthFilter {
    /// 创建新的深度过滤器
    pub fn new(max_depth: usize) -> Self {
        Self {
            max_depth,
            min_depth: 1,
            action: DepthAction::Truncate,
        }
    }

    /// 创建带最小深度的过滤器
    pub fn with_min_depth(mut self, min_depth: usize) -> Self {
        self.min_depth = min_depth;
        self.action = DepthAction::Filter;
        self
    }

    /// 设置为过滤模式
    pub fn filter_mode(mut self) -> Self {
        self.action = DepthAction::Filter;
        self
    }

    /// 设置为截断模式
    pub fn truncate_mode(mut self) -> Self {
        self.action = DepthAction::Truncate;
        self
    }

    /// 截断堆栈
    pub fn truncate(&self, stack: &mut CollectedStack) {
        if stack.frames.len() > self.max_depth {
            stack.frames.truncate(self.max_depth);
            stack.user_stack_depth = stack.frames.iter()
                .filter(|f| f.frame_type == FrameType::User).count();
            stack.kernel_stack_depth = stack.frames.iter()
                .filter(|f| f.frame_type == FrameType::Kernel).count();
        }
    }
}

impl StackFilter for DepthFilter {
    fn should_include(&self, stack: &CollectedStack) -> bool {
        match self.action {
            DepthAction::Filter => {
                stack.total_depth() >= self.min_depth && stack.total_depth() <= self.max_depth
            }
            DepthAction::Truncate => stack.total_depth() >= self.min_depth,
        }
    }

    fn should_include_frame(&self, _frame: &ResolvedFrame) -> bool {
        true
    }

    fn name(&self) -> &str {
        "DepthFilter"
    }

    fn description(&self) -> String {
        match self.action {
            DepthAction::Truncate => "Truncates stacks exceeding maximum depth".to_string(),
            DepthAction::Filter => "Filters stacks outside depth range".to_string(),
        }
    }
}

/// 进程 ID 过滤器
///
/// 只包含或排除特定进程的堆栈。
pub struct ProcessFilter {
    /// 进程 ID 集合
    process_ids: HashSet<ProcessId>,
    /// 是否为白名单模式（只包含列表中的进程）
    whitelist: bool,
}

impl ProcessFilter {
    /// 创建白名单过滤器（只包含指定进程）
    pub fn whitelist(process_ids: Vec<ProcessId>) -> Self {
        Self {
            process_ids: process_ids.into_iter().collect(),
            whitelist: true,
        }
    }

    /// 创建黑名单过滤器（排除指定进程）
    pub fn blacklist(process_ids: Vec<ProcessId>) -> Self {
        Self {
            process_ids: process_ids.into_iter().collect(),
            whitelist: false,
        }
    }

    /// 添加进程 ID
    pub fn add_process(&mut self, pid: ProcessId) -> &mut Self {
        self.process_ids.insert(pid);
        self
    }

    /// 移除进程 ID
    pub fn remove_process(&mut self, pid: ProcessId) -> &mut Self {
        self.process_ids.remove(&pid);
        self
    }
}

impl StackFilter for ProcessFilter {
    fn should_include(&self, stack: &CollectedStack) -> bool {
        let in_list = self.process_ids.contains(&stack.process_id);

        if self.whitelist {
            in_list
        } else {
            !in_list
        }
    }

    fn should_include_frame(&self, _frame: &ResolvedFrame) -> bool {
        true
    }

    fn name(&self) -> &str {
        if self.whitelist {
            "ProcessWhitelistFilter"
        } else {
            "ProcessBlacklistFilter"
        }
    }

    fn description(&self) -> String {
        if self.whitelist {
            "Only includes stacks from specified processes".to_string()
        } else {
            "Excludes stacks from specified processes".to_string()
        }
    }
}

/// 内核堆栈过滤器
///
/// 过滤掉纯内核堆栈。
pub struct KernelStackFilter {
    /// 是否允许混合堆栈（用户+内核）
    allow_mixed: bool,
    /// 是否完全禁用内核帧
    disable_kernel_frames: bool,
}

impl KernelStackFilter {
    /// 创建新的内核堆栈过滤器
    pub fn new() -> Self {
        Self {
            allow_mixed: true,
            disable_kernel_frames: false,
        }
    }

    /// 设置是否允许混合堆栈
    pub fn allow_mixed(mut self, allow: bool) -> Self {
        self.allow_mixed = allow;
        self
    }

    /// 设置是否禁用内核帧
    pub fn disable_kernel(mut self, disable: bool) -> Self {
        self.disable_kernel_frames = disable;
        self
    }
}

impl Default for KernelStackFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl StackFilter for KernelStackFilter {
    fn should_include(&self, stack: &CollectedStack) -> bool {
        if stack.is_kernel_stack && !self.allow_mixed {
            return false;
        }
        true
    }

    fn should_include_frame(&self, frame: &ResolvedFrame) -> bool {
        if self.disable_kernel_frames {
            frame.frame_type != FrameType::Kernel
        } else {
            true
        }
    }

    fn name(&self) -> &str {
        "KernelStackFilter"
    }

    fn description(&self) -> String {
        "Filters kernel stacks and/or frames".to_string()
    }
}

/// 组合过滤器
///
/// 组合多个过滤器，可以设置逻辑关系（AND/OR）。
pub struct CompositeFilter {
    /// 子过滤器列表
    filters: Vec<Box<dyn StackFilter>>,
    /// 组合模式
    mode: FilterCombinationMode,
}

/// 过滤器组合模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterCombinationMode {
    /// 所有过滤器都必须通过（AND）
    All,
    /// 任一过滤器通过即可（OR）
    Any,
}

impl CompositeFilter {
    /// 创建新的组合过滤器（AND 模式）
    pub fn new(filters: Vec<Box<dyn StackFilter>>) -> Self {
        Self {
            filters,
            mode: FilterCombinationMode::All,
        }
    }

    /// 创建 OR 模式的组合过滤器
    pub fn any(filters: Vec<Box<dyn StackFilter>>) -> Self {
        Self {
            filters,
            mode: FilterCombinationMode::Any,
        }
    }

    /// 添加过滤器
    pub fn add_filter(&mut self, filter: Box<dyn StackFilter>) -> &mut Self {
        self.filters.push(filter);
        self
    }

    /// 设置组合模式
    pub fn set_mode(&mut self, mode: FilterCombinationMode) -> &mut Self {
        self.mode = mode;
        self
    }
}

impl StackFilter for CompositeFilter {
    fn should_include(&self, stack: &CollectedStack) -> bool {
        match self.mode {
            FilterCombinationMode::All => {
                self.filters.iter().all(|f| f.should_include(stack))
            }
            FilterCombinationMode::Any => {
                self.filters.iter().any(|f| f.should_include(stack))
            }
        }
    }

    fn should_include_frame(&self, frame: &ResolvedFrame) -> bool {
        match self.mode {
            FilterCombinationMode::All => {
                self.filters.iter().all(|f| f.should_include_frame(frame))
            }
            FilterCombinationMode::Any => {
                self.filters.iter().any(|f| f.should_include_frame(frame))
            }
        }
    }

    fn name(&self) -> &str {
        "CompositeFilter"
    }

    fn description(&self) -> String {
        let names: Vec<_> = self.filters.iter().map(|f| f.name()).collect();
        let mode_str = match self.mode {
            FilterCombinationMode::All => "AND",
            FilterCombinationMode::Any => "OR",
        };
        format!("Combines filters [{}] with {}", names.join(", "), mode_str)
    }
}

/// 函数名过滤器
///
/// 基于函数名的包含/排除过滤。
pub struct FunctionNameFilter {
    /// 模式列表
    patterns: Vec<String>,
    /// 是否为白名单
    whitelist: bool,
    /// 是否区分大小写
    case_sensitive: bool,
}

impl FunctionNameFilter {
    /// 创建新的函数名过滤器
    pub fn new(patterns: Vec<String>) -> Self {
        Self {
            patterns: patterns.into_iter()
                .map(|p| p.to_lowercase())
                .collect(),
            whitelist: true,
            case_sensitive: false,
        }
    }

    /// 设置为黑名单模式
    pub fn blacklist(mut self) -> Self {
        self.whitelist = false;
        self
    }

    /// 设置是否区分大小写
    pub fn case_sensitive(mut self, sensitive: bool) -> Self {
        self.case_sensitive = sensitive;
        if !sensitive {
            self.patterns = self.patterns.iter()
                .map(|p| p.to_lowercase())
                .collect();
        }
        self
    }

    /// 添加模式
    pub fn add_pattern(&mut self, pattern: impl Into<String>) -> &mut Self {
        let pattern = pattern.into();
        self.patterns.push(if self.case_sensitive {
            pattern
        } else {
            pattern.to_lowercase()
        });
        self
    }

    /// 检查函数名是否匹配
    fn matches(&self, function_name: &str) -> bool {
        let name = if self.case_sensitive {
            function_name.to_string()
        } else {
            function_name.to_lowercase()
        };

        self.patterns.iter().any(|p| name.contains(p))
    }
}

impl StackFilter for FunctionNameFilter {
    fn should_include(&self, stack: &CollectedStack) -> bool {
        let has_match = stack.frames.iter().any(|f| {
            f.symbol.as_ref().map(|s| self.matches(&s.name)).unwrap_or(false)
        });

        if self.whitelist {
            has_match
        } else {
            !has_match
        }
    }

    fn should_include_frame(&self, frame: &ResolvedFrame) -> bool {
        let matches = frame.symbol.as_ref()
            .map(|s| self.matches(&s.name))
            .unwrap_or(false);

        if self.whitelist {
            matches
        } else {
            !matches
        }
    }

    fn name(&self) -> &str {
        "FunctionNameFilter"
    }

    fn description(&self) -> String {
        if self.whitelist {
            "Includes stacks containing specified function names".to_string()
        } else {
            "Excludes stacks containing specified function names".to_string()
        }
    }
}

/// 应用过滤器到堆栈
///
/// 过滤堆栈中的帧并返回新的堆栈。
pub fn apply_filter(stack: &CollectedStack, filter: &dyn StackFilter) -> Option<CollectedStack> {
    if !filter.should_include(stack) {
        return None;
    }

    let filtered_frames: Vec<_> = stack.frames.iter()
        .filter(|f| filter.should_include_frame(f))
        .cloned()
        .collect();

    if filtered_frames.is_empty() {
        return None;
    }

    Some(CollectedStack::new(
        stack.process_id,
        stack.thread_id,
        stack.timestamp,
        filtered_frames,
    ))
}

/// 过滤器链构建器
///
/// 方便地构建过滤器链。
pub struct FilterChainBuilder {
    filters: Vec<Box<dyn StackFilter>>,
}

impl FilterChainBuilder {
    /// 创建新的构建器
    pub fn new() -> Self {
        Self {
            filters: Vec::new(),
        }
    }

    /// 添加系统模块过滤器
    pub fn exclude_system_modules(mut self) -> Self {
        self.filters.push(Box::new(SystemModuleFilter::new()));
        self
    }

    /// 添加深度过滤器
    pub fn max_depth(mut self, depth: usize) -> Self {
        self.filters.push(Box::new(DepthFilter::new(depth)));
        self
    }

    /// 添加进程过滤器
    pub fn include_processes(mut self, pids: Vec<ProcessId>) -> Self {
        self.filters.push(Box::new(ProcessFilter::whitelist(pids)));
        self
    }

    /// 排除进程
    pub fn exclude_processes(mut self, pids: Vec<ProcessId>) -> Self {
        self.filters.push(Box::new(ProcessFilter::blacklist(pids)));
        self
    }

    /// 添加自定义过滤器
    pub fn add_filter(mut self, filter: Box<dyn StackFilter>) -> Self {
        self.filters.push(filter);
        self
    }

    /// 构建组合过滤器
    pub fn build(self) -> CompositeFilter {
        CompositeFilter::new(self.filters)
    }
}

impl Default for FilterChainBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ModuleInfo, SymbolInfo};

    fn create_test_stack() -> CollectedStack {
        let frames = vec![
            ResolvedFrame {
                address: 0x00401000,
                symbol: Some(SymbolInfo {
                    address: 0x00401000,
                    name: "main".to_string(),
                    module: Some("test.exe".to_string()),
                    source_file: None,
                    line_number: None,
                }),
                module: Some(ModuleInfo::new(0x00400000, 0x10000, "test.exe")),
                frame_type: FrameType::User,
            },
            ResolvedFrame {
                address: 0x7FF812345678,
                symbol: Some(SymbolInfo {
                    address: 0x7FF812345678,
                    name: "CreateFileW".to_string(),
                    module: Some("kernel32.dll".to_string()),
                    source_file: None,
                    line_number: None,
                }),
                module: Some(ModuleInfo::new(0x7FF812300000, 0x100000, "kernel32.dll")),
                frame_type: FrameType::User,
            },
        ];

        CollectedStack::new(1234, 5678, 1000, frames)
    }

    #[test]
    fn test_system_module_filter() {
        let filter = SystemModuleFilter::new();
        let stack = create_test_stack();

        // 堆栈应该被包含（不全由系统模块组成）
        assert!(filter.should_include(&stack));

        // kernel32.dll 的帧应该被过滤掉
        assert!(filter.should_include_frame(&stack.frames[0]));
        assert!(!filter.should_include_frame(&stack.frames[1]));
    }

    #[test]
    fn test_depth_filter() {
        let filter = DepthFilter::new(10);
        let stack = create_test_stack();

        assert!(filter.should_include(&stack));

        let deep_filter = DepthFilter::new(1).filter_mode();
        assert!(!deep_filter.should_include(&stack));
    }

    #[test]
    fn test_process_filter() {
        let filter = ProcessFilter::whitelist(vec![1234]);
        let stack = create_test_stack();

        assert!(filter.should_include(&stack));

        let filter2 = ProcessFilter::blacklist(vec![1234]);
        assert!(!filter2.should_include(&stack));
    }

    #[test]
    fn test_composite_filter() {
        let filter1: Box<dyn StackFilter> = Box::new(SystemModuleFilter::new());
        let filter2: Box<dyn StackFilter> = Box::new(DepthFilter::new(10));

        let composite = CompositeFilter::new(vec![filter1, filter2]);
        let stack = create_test_stack();

        assert!(composite.should_include(&stack));
    }

    #[test]
    fn test_filter_chain_builder() {
        let filter = FilterChainBuilder::new()
            .exclude_system_modules()
            .max_depth(64)
            .build();

        let stack = create_test_stack();
        assert!(filter.should_include(&stack));
    }

    #[test]
    fn test_apply_filter() {
        let filter = SystemModuleFilter::new();
        let stack = create_test_stack();

        let result = apply_filter(&stack, &filter);
        assert!(result.is_some());

        let filtered = result.unwrap();
        assert_eq!(filtered.frames.len(), 1); // kernel32.dll 的帧被过滤
    }
}
