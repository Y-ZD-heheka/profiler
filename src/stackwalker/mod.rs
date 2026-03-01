//! 堆栈遍历模块 (Stack Walker)
//!
//! 提供调用堆栈的展开、解析和收集功能，将 ETW 采样事件中的原始地址
//! 转换为带符号信息的完整调用堆栈。
//!
//! # 主要组件
//!
//! - [`StackWalker`]: 堆栈遍历的核心 trait
//! - [`StackUnwinder`]: 堆栈地址展开 trait
//! - [`StackCollector`]: 整合展开和解析的堆栈收集器
//! - [`StackManager`]: 管理堆栈收集的生命周期
//! - [`StackFilter`]: 堆栈过滤接口
//!
//! # 使用示例
//!
//! ```rust,no_run
//! use profiler::stackwalker::{StackManager, StackManagerConfig};
//! use profiler::symbols::SymbolManager;
//! use std::sync::Arc;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // 创建符号管理器
//! let symbol_manager = Arc::new(SymbolManager::new());
//!
//! // 配置堆栈管理器
//! let config = StackManagerConfig::default();
//! let mut stack_manager = StackManager::new(config);
//!
//! // 初始化
//! stack_manager.initialize(symbol_manager)?;
//!
//! // 处理采样事件
//! // stack_manager.on_sample(&sample_event)?;
//! # Ok(())
//! # }
//! ```

use crate::error::Result;
use crate::types::{Address, CallStack, ModuleInfo, ProcessId, StackFrame, SymbolInfo, ThreadId, Timestamp};

// 子模块定义
mod collector;
mod filter;
mod manager;
mod resolver;
mod unwinder;

// 公开导出
pub use collector::{CollectedStack, FrameType, ResolvedFrame, StackCollector};
pub use filter::{CompositeFilter, DepthFilter, StackFilter, SystemModuleFilter};
pub use manager::{StackManager, StackManagerConfig, StackManagerStats};
pub use resolver::{ResolveStats, StackResolver};
pub use unwinder::{EtwStackUnwinder, UnwindInfo, UnwindOp, UnwindOptions, UnwindStats};

/// 堆栈遍历器 Trait
///
/// 定义遍历调用堆栈的标准接口，实现者可以提供不同的堆栈遍历策略。
///
/// # 线程安全
///
/// 该 trait 要求实现类型是 `Send + Sync` 的，因为堆栈遍历可能在多线程环境中使用。
pub trait StackWalker: Send + Sync {
    /// 从进程和线程 ID 遍历堆栈
    ///
    /// # 参数
    /// - `process_id`: 目标进程 ID
    /// - `thread_id`: 目标线程 ID
    ///
    /// # 返回
    /// 遍历得到的地址列表，从栈顶到栈底排序
    fn walk_stack(&self, process_id: ProcessId, thread_id: ThreadId) -> Result<Vec<Address>>;

    /// 从 CPU 上下文遍历堆栈
    ///
    /// # 参数
    /// - `process_id`: 目标进程 ID
    /// - `context`: CPU 上下文（寄存器状态）
    ///
    /// # 返回
    /// 遍历得到的地址列表
    fn walk_stack_from_context(
        &self,
        process_id: ProcessId,
        context: &windows::Win32::System::Diagnostics::Debug::CONTEXT,
    ) -> Result<Vec<Address>>;

    /// 设置最大堆栈深度
    fn set_max_depth(&mut self, depth: usize);

    /// 获取当前最大堆栈深度
    fn max_depth(&self) -> usize;
}

/// 堆栈展开器 Trait（从 unwinder 模块重新导出）
pub use unwinder::StackUnwinder;

/// 内核地址检测工具函数
///
/// 在 Windows x64 系统中，内核地址空间通常从 0xFFFF000000000000 开始
pub fn is_kernel_address(address: Address) -> bool {
    // Windows x64 内核地址空间从 0xFFFF000000000000 开始
    address >= 0xFFFF000000000000
}

/// 用户态地址检测工具函数
pub fn is_user_address(address: Address) -> bool {
    !is_kernel_address(address)
}

/// 将地址分类为内核态或用户态
pub fn classify_address(address: Address) -> AddressType {
    if is_kernel_address(address) {
        AddressType::Kernel
    } else {
        AddressType::User
    }
}

/// 地址类型枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressType {
    /// 用户态地址
    User,
    /// 内核态地址
    Kernel,
}

/// 系统模块列表
pub const SYSTEM_MODULES: &[&str] = &[
    "ntdll.dll",
    "kernel32.dll",
    "kernelbase.dll",
    "ntoskrnl.exe",
    "hal.dll",
    "win32k.sys",
    "win32kbase.sys",
    "win32kfull.sys",
];

/// 检查模块名是否为系统模块
pub fn is_system_module(module_name: &str) -> bool {
    let lower_name = module_name.to_lowercase();
    SYSTEM_MODULES.iter().any(|&m| lower_name.contains(m))
}

/// 堆栈遍历错误上下文
#[derive(Debug, Clone)]
pub struct StackWalkContext {
    /// 进程 ID
    pub process_id: ProcessId,
    /// 线程 ID
    pub thread_id: ThreadId,
    /// 操作描述
    pub operation: String,
}

impl StackWalkContext {
    /// 创建新的上下文
    pub fn new(process_id: ProcessId, thread_id: ThreadId, operation: impl Into<String>) -> Self {
        Self {
            process_id,
            thread_id,
            operation: operation.into(),
        }
    }
}

/// 堆栈回调 trait
///
/// 用于接收收集到的堆栈信息
pub trait StackCallback: Send + Sync {
    /// 处理收集到的堆栈
    fn on_stack_collected(&mut self, stack: &CollectedStack);

    /// 处理堆栈收集错误
    fn on_stack_error(&mut self, error: &crate::error::ProfilerError, context: &StackWalkContext);
}

/// 默认的堆栈回调（空实现）
pub struct DefaultStackCallback;

impl StackCallback for DefaultStackCallback {
    fn on_stack_collected(&mut self, _stack: &CollectedStack) {}

    fn on_stack_error(&mut self, _error: &crate::error::ProfilerError, _context: &StackWalkContext) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_classification() {
        assert!(is_kernel_address(0xFFFF000000000000));
        assert!(is_kernel_address(0xFFFFF80000000000));
        assert!(is_user_address(0x00007FF123456789));
        assert!(is_user_address(0x0000000000401000));
        assert_eq!(classify_address(0xFFFF000000000000), AddressType::Kernel);
        assert_eq!(classify_address(0x00007FF123456789), AddressType::User);
    }

    #[test]
    fn test_system_module_detection() {
        assert!(is_system_module("ntdll.dll"));
        assert!(is_system_module("NTDLL.DLL"));
        assert!(is_system_module("C:\\Windows\\System32\\kernel32.dll"));
        assert!(!is_system_module("myapp.exe"));
        assert!(!is_system_module("user.dll"));
    }

    #[test]
    fn test_stack_walk_context() {
        let ctx = StackWalkContext::new(1234, 5678, "test operation");
        assert_eq!(ctx.process_id, 1234);
        assert_eq!(ctx.thread_id, 5678);
        assert_eq!(ctx.operation, "test operation");
    }
}
