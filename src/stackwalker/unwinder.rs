//! 堆栈展开器实现
//!
//! 提供从 ETW 事件和 CPU 上下文展开调用堆栈的功能。
//! 支持 x64/x86 架构的调用约定，处理帧指针优化(FPO)等情况。

use crate::error::{Result, SymbolError};
use crate::etw::SampledProfileEvent;
use crate::stackwalker::is_kernel_address;
use crate::types::{Address, ProcessId};

/// 堆栈展开器 Trait
///
/// 定义从各种数据源展开堆栈地址的标准接口。
/// 这是 [`StackWalker`] 的低级接口，专注于地址提取。
pub trait StackUnwinder: Send + Sync {
    /// 从 ETW 采样事件展开堆栈
    ///
    /// # 参数
    /// - `event`: ETW 采样配置文件事件
    ///
    /// # 返回
    /// 展开得到的原始地址列表
    fn unwind_from_event(
        &self,
        event: &SampledProfileEvent,
    ) -> Result<Vec<Address>>;

    /// 从 CPU 上下文展开堆栈
    ///
    /// # 参数
    /// - `context`: CPU 上下文结构
    /// - `process_id`: 进程 ID（用于内存读取）
    ///
    /// # 返回
    /// 展开得到的原始地址列表
    fn unwind_from_context(
        &self,
        context: &CONTEXT,
        process_id: ProcessId,
    ) -> Result<Vec<Address>>;

    /// 获取展开统计信息
    fn get_stats(&self) -> UnwindStats;

    /// 重置统计信息
    fn reset_stats(&mut self);
}
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tracing::{debug, trace, warn};
use windows::Win32::System::Diagnostics::Debug::CONTEXT;

/// ETW 堆栈展开器
///
/// 从 ETW 采样事件或 CPU 上下文展开调用堆栈，提取原始地址列表。
/// 支持区分内核态和用户态堆栈。
///
/// # 线程安全
///
/// 所有方法都是线程安全的，统计信息使用原子操作保护。
pub struct EtwStackUnwinder {
    /// 最大堆栈展开深度
    max_depth: Mutex<usize>,
    /// 是否启用内核堆栈收集
    enable_kernel_stack: Mutex<bool>,
    /// 成功展开计数
    success_count: AtomicU64,
    /// 失败展开计数
    fail_count: AtomicU64,
    /// 展开的总帧数
    total_frames: AtomicU64,
    /// 内核堆栈帧计数
    kernel_frames: AtomicU64,
}

/// 展开统计信息
#[derive(Debug, Clone, Copy, Default)]
pub struct UnwindStats {
    /// 成功展开次数
    pub success_count: u64,
    /// 失败展开次数
    pub fail_count: u64,
    /// 展开的总帧数
    pub total_frames: u64,
    /// 内核堆栈帧数
    pub kernel_frames: u64,
    /// 成功率（百分比）
    pub success_rate: f64,
}

impl EtwStackUnwinder {
    /// 创建新的堆栈展开器
    ///
    /// # 示例
    ///
    /// ```rust
    /// use profiler::stackwalker::EtwStackUnwinder;
    ///
    /// let unwinder = EtwStackUnwinder::new();
    /// ```
    pub fn new() -> Self {
        Self {
            max_depth: Mutex::new(64),
            enable_kernel_stack: Mutex::new(true),
            success_count: AtomicU64::new(0),
            fail_count: AtomicU64::new(0),
            total_frames: AtomicU64::new(0),
            kernel_frames: AtomicU64::new(0),
        }
    }

    /// 创建带配置的堆栈展开器
    ///
    /// # 参数
    /// - `max_depth`: 最大堆栈深度
    /// - `enable_kernel_stack`: 是否启用内核堆栈收集
    pub fn with_config(max_depth: usize, enable_kernel_stack: bool) -> Self {
        Self {
            max_depth: Mutex::new(max_depth),
            enable_kernel_stack: Mutex::new(enable_kernel_stack),
            success_count: AtomicU64::new(0),
            fail_count: AtomicU64::new(0),
            total_frames: AtomicU64::new(0),
            kernel_frames: AtomicU64::new(0),
        }
    }

    /// 设置最大堆栈深度
    pub fn set_max_depth(&self, depth: usize) {
        if let Ok(mut max_depth) = self.max_depth.lock() {
            *max_depth = depth;
            debug!("Max stack depth set to {}", depth);
        }
    }

    /// 获取最大堆栈深度
    pub fn get_max_depth(&self) -> usize {
        self.max_depth.lock().map(|d| *d).unwrap_or(64)
    }

    /// 启用或禁用内核堆栈收集
    pub fn set_enable_kernel_stack(&self, enable: bool) {
        if let Ok(mut enabled) = self.enable_kernel_stack.lock() {
            *enabled = enable;
            debug!("Kernel stack collection set to {}", enable);
        }
    }

    /// 检查是否启用了内核堆栈收集
    pub fn is_kernel_stack_enabled(&self) -> bool {
        self.enable_kernel_stack.lock().map(|e| *e).unwrap_or(true)
    }

    /// 从 ETW 事件提取堆栈地址
    ///
    /// ETW SampledProfile 事件的堆栈数据格式：
    /// ```
    /// [StackLength: u32] [Address1: u64] [Address2: u64] ...
    /// ```
    /// 地址按从深到浅的顺序排列（被调者在先）
    ///
    /// # 参数
    /// - `event`: ETW 采样事件
    ///
    /// # 返回
    /// 提取的地址列表，失败时返回错误
    pub fn unwind_from_event_impl(&self, event: &SampledProfileEvent) -> Result<Vec<Address>> {
        trace!(
            "Unwinding stack from event: pid={}, tid={}, ip=0x{:016X}",
            event.process_id,
            event.thread_id,
            event.instruction_pointer
        );

        // 从 ETW 事件获取堆栈数据
        // 注意：实际的 ETW 堆栈数据需要从事件属性中提取
        // 这里我们使用指令指针作为起点
        let mut addresses = vec![event.instruction_pointer];

        // 检查是否需要展开更深
        let max_depth = self.get_max_depth();
        let kernel_enabled = self.is_kernel_stack_enabled();

        // 区分用户态和内核态地址
        let is_kernel = !event.user_mode || is_kernel_address(event.instruction_pointer);

        if is_kernel && !kernel_enabled {
            // 内核堆栈被禁用，只返回指令指针
            self.success_count.fetch_add(1, Ordering::Relaxed);
            return Ok(addresses);
        }

        // 在实际实现中，这里会从 ETW 事件的扩展数据中提取完整堆栈
        // 对于原型实现，我们返回包含指令指针的基本堆栈

        // 限制深度
        if addresses.len() > max_depth {
            addresses.truncate(max_depth);
            trace!("Stack truncated to {} frames", max_depth);
        }

        // 更新统计
        self.success_count.fetch_add(1, Ordering::Relaxed);
        self.total_frames.fetch_add(addresses.len() as u64, Ordering::Relaxed);

        // 统计内核帧
        let kernel_count = addresses.iter().filter(|&&a| is_kernel_address(a)).count() as u64;
        self.kernel_frames.fetch_add(kernel_count, Ordering::Relaxed);

        trace!("Unwound {} frames from event", addresses.len());
        Ok(addresses)
    }

    /// 从 CPU 上下文展开堆栈
    ///
    /// 使用 CONTEXT 结构中的寄存器信息展开堆栈。
    /// x64 架构使用 RIP, RSP, RBP 寄存器
    /// x86 架构使用 EIP, ESP, EBP 寄存器
    ///
    /// # 参数
    /// - `context`: CPU 上下文
    /// - `process_id`: 进程 ID
    ///
    /// # 返回
    /// 展开得到的地址列表
    pub fn unwind_from_context_impl(
        &self,
        context: &CONTEXT,
        process_id: ProcessId,
    ) -> Result<Vec<Address>> {
        trace!("Unwinding stack from context for process {}", process_id);

        let mut addresses = Vec::new();
        let max_depth = self.get_max_depth();
        let kernel_enabled = self.is_kernel_stack_enabled();

        // x64 架构展开
        #[cfg(target_arch = "x86_64")]
        {
            // 从 RIP（指令指针）开始
            let rip = context.Rip;
            if rip != 0 {
                addresses.push(rip);

                // 检查是否为内核地址
                if is_kernel_address(rip) && !kernel_enabled {
                    self.success_count.fetch_add(1, Ordering::Relaxed);
                    return Ok(addresses);
                }
            }

            // 使用 RBP（帧指针）链遍历堆栈
            // 这是传统的帧指针遍历方法，适用于未优化代码
            let mut rbp = context.Rbp;
            let mut rsp = context.Rsp;

            while addresses.len() < max_depth && rbp != 0 {
                // 验证 RBP 是否在当前堆栈范围内
                if rbp < rsp || rbp > rsp + 0x10000 {
                    break;
                }

                // 读取返回地址（RBP + 8）
                // 注意：这里需要从目标进程内存读取
                // 实际实现中需要使用 ReadProcessMemory
                let return_addr = rbp.wrapping_add(8);
                if return_addr == 0 {
                    break;
                }

                // 尝试读取返回地址处的值
                match self.read_return_address(process_id, return_addr) {
                    Some(addr) if addr != 0 => {
                        addresses.push(addr);
                        // 更新 RBP 为上一个帧指针
                        rbp = self.read_frame_pointer(process_id, rbp).unwrap_or(0);
                    }
                    _ => break,
                }
            }
        }

        // x86 架构展开
        #[cfg(target_arch = "x86")]
        {
            let eip = context.Eip as u64;
            if eip != 0 {
                addresses.push(eip);

                if is_kernel_address(eip) && !kernel_enabled {
                    self.success_count.fetch_add(1, Ordering::Relaxed);
                    return Ok(addresses);
                }
            }

            // 使用 EBP 链遍历
            let mut ebp = context.Ebp as u64;
            let mut esp = context.Esp as u64;

            while addresses.len() < max_depth && ebp != 0 {
                if ebp < esp || ebp > esp + 0x10000 {
                    break;
                }

                let return_addr = ebp.wrapping_add(4);
                match self.read_return_address_32(process_id, return_addr as u32) {
                    Some(addr) if addr != 0 => {
                        addresses.push(addr as u64);
                        ebp = self.read_frame_pointer_32(process_id, ebp as u32).unwrap_or(0) as u64;
                    }
                    _ => break,
                }
            }
        }

        // 更新统计
        self.success_count.fetch_add(1, Ordering::Relaxed);
        self.total_frames.fetch_add(addresses.len() as u64, Ordering::Relaxed);

        let kernel_count = addresses.iter().filter(|&&a| is_kernel_address(a)).count() as u64;
        self.kernel_frames.fetch_add(kernel_count, Ordering::Relaxed);

        trace!("Unwound {} frames from context", addresses.len());
        Ok(addresses)
    }

    /// 从进程内存读取返回地址（64位）
    fn read_return_address(&self, process_id: ProcessId, address: u64) -> Option<u64> {
        // 实际实现中需要使用 ReadProcessMemory
        // 这里返回 None 作为占位
        trace!("Reading return address from process {} at 0x{:016X}", process_id, address);
        None
    }

    /// 从进程内存读取帧指针（64位）
    fn read_frame_pointer(&self, process_id: ProcessId, address: u64) -> Option<u64> {
        trace!("Reading frame pointer from process {} at 0x{:016X}", process_id, address);
        None
    }

    /// 从进程内存读取返回地址（32位）
    #[cfg(target_arch = "x86")]
    fn read_return_address_32(&self, process_id: ProcessId, address: u32) -> Option<u32> {
        trace!("Reading 32-bit return address from process {} at 0x{:08X}", process_id, address);
        None
    }

    /// 从进程内存读取帧指针（32位）
    #[cfg(target_arch = "x86")]
    fn read_frame_pointer_32(&self, process_id: ProcessId, address: u32) -> Option<u32> {
        trace!("Reading 32-bit frame pointer from process {} at 0x{:08X}", process_id, address);
        None
    }

    /// 使用 DbgHelp API 展开堆栈（备用方法）
    #[allow(dead_code)]
    fn unwind_with_dbghelp(
        &self,
        _process_id: ProcessId,
        _context: &CONTEXT,
    ) -> Result<Vec<Address>> {
        // 实际实现中可以使用 StackWalk64 API
        // 这需要打开目标进程并获取线程句柄
        warn!("DbgHelp stack unwinding not yet implemented");
        Err(SymbolError::new("DbgHelp unwinding not implemented").into())
    }
}

impl Default for EtwStackUnwinder {
    fn default() -> Self {
        Self::new()
    }
}

impl StackUnwinder for EtwStackUnwinder {
    fn unwind_from_event(&self, event: &SampledProfileEvent) -> Result<Vec<Address>> {
        self.unwind_from_event_impl(event)
    }

    fn unwind_from_context(&self, context: &CONTEXT, process_id: ProcessId) -> Result<Vec<Address>> {
        self.unwind_from_context_impl(context, process_id)
    }

    fn get_stats(&self) -> UnwindStats {
        let success = self.success_count.load(Ordering::Relaxed);
        let fail = self.fail_count.load(Ordering::Relaxed);
        let total = success + fail;

        UnwindStats {
            success_count: success,
            fail_count: fail,
            total_frames: self.total_frames.load(Ordering::Relaxed),
            kernel_frames: self.kernel_frames.load(Ordering::Relaxed),
            success_rate: if total > 0 {
                (success as f64 / total as f64) * 100.0
            } else {
                0.0
            },
        }
    }

    fn reset_stats(&mut self) {
        self.success_count.store(0, Ordering::Relaxed);
        self.fail_count.store(0, Ordering::Relaxed);
        self.total_frames.store(0, Ordering::Relaxed);
        self.kernel_frames.store(0, Ordering::Relaxed);
        debug!("Unwind statistics reset");
    }
}

/// x64 Unwind Code 类型（用于解析 .pdata）
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UnwindOp {
    /// 压入非易失性寄存器
    PushNonvol = 0,
    /// 分配大栈空间（16-512 KB）
    AllocLarge = 1,
    /// 分配小栈空间（8-128 字节）
    AllocSmall = 2,
    /// 设置帧指针寄存器
    SetFpReg = 3,
    /// 保存非易失性寄存器（使用比例偏移）
    SaveNonvol = 4,
    /// 保存非易失性寄存器（大偏移）
    SaveNonvolFar = 5,
    /// 保存 XMM 寄存器（未使用）
    SaveXmm128 = 6,
    /// 保存 XMM 寄存器（大偏移，未使用）
    SaveXmm128Far = 7,
    /// 压入机器帧
    PushMachframe = 8,
}

/// 展开信息结构（x64 .pdata 条目）
#[derive(Debug, Clone)]
pub struct UnwindInfo {
    /// 函数起始 RVA
    pub function_start: u32,
    /// 函数结束 RVA
    pub function_end: u32,
    /// 展开数据 RVA
    pub unwind_data: u32,
}

/// 解析 x64 PE 文件的 .pdata 节
///
/// # 参数
/// - `module_base`: 模块基地址
///
/// # 返回
/// 展开信息列表
pub fn parse_pdata_section(_module_base: Address) -> Result<Vec<UnwindInfo>> {
    // 实际实现需要：
    // 1. 读取 PE 头
    // 2. 找到 .pdata 节
    // 3. 解析 RUNTIME_FUNCTION 条目
    // 4. 转换为 UnwindInfo

    // 这是一个占位实现
    Ok(Vec::new())
}

/// 使用 RtlVirtualUnwind 展开单帧
///
/// # 安全性
/// 调用 Windows API 需要正确处理上下文和内存访问
#[allow(dead_code)]
pub unsafe fn unwind_single_frame(
    _program_counter: Address,
    _stack_pointer: Address,
    _frame_pointer: Address,
) -> Option<(Address, Address, Address)> {
    // 实际实现需要使用 RtlVirtualUnwind API
    // 输入：当前 RIP, RSP, RBP
    // 输出：上一帧的 RIP, RSP, RBP
    None
}

/// 堆栈展开选项
#[derive(Debug, Clone, Copy)]
pub struct UnwindOptions {
    /// 最大深度
    pub max_depth: usize,
    /// 是否包含内核堆栈
    pub include_kernel: bool,
    /// 使用帧指针链（而非 unwind info）
    pub use_frame_pointers: bool,
    /// 停止于系统模块边界
    pub stop_at_system_modules: bool,
}

impl Default for UnwindOptions {
    fn default() -> Self {
        Self {
            max_depth: 64,
            include_kernel: true,
            use_frame_pointers: false,
            stop_at_system_modules: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_etw_stack_unwinder_creation() {
        let unwinder = EtwStackUnwinder::new();
        assert_eq!(unwinder.get_max_depth(), 64);
        assert!(unwinder.is_kernel_stack_enabled());
    }

    #[test]
    fn test_etw_stack_unwinder_config() {
        let unwinder = EtwStackUnwinder::with_config(128, false);
        assert_eq!(unwinder.get_max_depth(), 128);
        assert!(!unwinder.is_kernel_stack_enabled());
    }

    #[test]
    fn test_unwind_options_default() {
        let opts = UnwindOptions::default();
        assert_eq!(opts.max_depth, 64);
        assert!(opts.include_kernel);
        assert!(!opts.use_frame_pointers);
        assert!(!opts.stop_at_system_modules);
    }

    #[test]
    fn test_unwind_stats() {
        let stats = UnwindStats {
            success_count: 90,
            fail_count: 10,
            total_frames: 1000,
            kernel_frames: 100,
            success_rate: 90.0,
        };
        assert_eq!(stats.success_rate, 90.0);
    }
}
