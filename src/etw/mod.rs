//! ETW (Event Trace for Windows) 性能分析模块
//!
//! 该模块提供基于 Windows ETW API 的高性能事件采集功能，
//! 支持 CPU 采样、进程/线程生命周期跟踪和模块加载事件捕获。
//!
//! # 主要组件
//!
//! - [`EtwSession`]: 管理单个 ETW 会话的生命周期
//! - [`EtwController`]: 协调多个会话和事件处理
//! - [`EventProcessor`]: 处理和转换 ETW 事件
//! - [`KernelProviderFlags`]: 内核提供者标志位定义
//!
//! # 使用示例
//!
//! ```no_run
//! use etw_profiler::etw::{EtwController, EtwSession, KernelProviderFlags};
//! use etw_profiler::config::ProfilerConfig;
//!
//! async fn example() {
//!     let config = ProfilerConfig::default();
//!     let mut controller = EtwController::new(&config).unwrap();
//!     
//!     let mut session = controller.create_realtime_session("MySession").unwrap();
//!     session.enable_kernel_provider(KernelProviderFlags::PROFILE).unwrap();
//!     session.start().unwrap();
//!     
//!     // ... 性能分析 ...
//!     
//!     session.stop().unwrap();
//! }
//! ```

use crate::error::Result;
use crate::types::{Address, ProcessId, SampleEvent, ThreadId, Timestamp};
use std::path::Path;

// 子模块定义
mod controller;
mod processor;
mod provider;
mod session;

// 公开导出
pub use controller::EtwController;
pub use processor::{EventProcessor, ProcessedEvent};
pub use provider::{KernelProviderFlags, ProviderConfig};
pub use session::{EtwSession, SessionConfig, SessionMode};

// ============================================================================
// 核心 Trait 定义
// ============================================================================

/// ETW 提供者接口
///
/// 定义 ETW 事件提供者的基本行为，用于启用和配置特定的 ETW 提供者。
///
/// # 类型参数
///
/// - `Flags`: 提供者特定的标志位类型
pub trait EtwProvider {
    /// 标志位类型
    type Flags;

    /// 获取提供者的 GUID
    ///
    /// # 返回
    ///
    /// 返回该提供者的唯一标识符
    fn guid(&self) -> windows::core::GUID;

    /// 启用提供者
    ///
    /// # 参数
    ///
    /// - `flags`: 提供者特定的标志位
    /// - `level`: 日志级别 (0-255)
    ///
    /// # 错误
    ///
    /// 如果启用失败，返回 [`EtwError`](crate::error::EtwError)
    fn enable(&mut self, flags: Self::Flags, level: u8) -> Result<()>;

    /// 禁用提供者
    ///
    /// # 错误
    ///
    /// 如果禁用失败，返回 [`EtwError`](crate::error::EtwError)
    fn disable(&mut self) -> Result<()>;

    /// 检查提供者是否已启用
    fn is_enabled(&self) -> bool;
}

/// 事件处理器接口
///
/// 定义处理 ETW 事件回调的标准接口。实现者可以接收和
/// 处理各种 ETW 事件。
///
/// # 线程安全
///
/// 该 trait 要求实现类型是 `Send` 的，因为事件回调可能
/// 发生在不同的线程上。
pub trait EventHandler: Send {
    /// 处理采样事件
    ///
    /// # 参数
    ///
    /// - `event`: 采样事件数据
    fn on_sample(&mut self, event: &SampleEvent);

    /// 处理进程开始事件
    ///
    /// # 参数
    ///
    /// - `timestamp`: 事件时间戳
    /// - `pid`: 进程 ID
    /// - `name`: 进程名称（如果可用）
    fn on_process_start(&mut self, timestamp: Timestamp, pid: ProcessId, name: Option<&str>);

    /// 处理进程结束事件
    ///
    /// # 参数
    ///
    /// - `timestamp`: 事件时间戳
    /// - `pid`: 进程 ID
    /// - `exit_code`: 进程退出代码
    fn on_process_end(&mut self, timestamp: Timestamp, pid: ProcessId, exit_code: u32);

    /// 处理线程开始事件
    ///
    /// # 参数
    ///
    /// - `timestamp`: 事件时间戳
    /// - `pid`: 所属进程 ID
    /// - `tid`: 线程 ID
    fn on_thread_start(&mut self, timestamp: Timestamp, pid: ProcessId, tid: ThreadId);

    /// 处理线程结束事件
    ///
    /// # 参数
    ///
    /// - `timestamp`: 事件时间戳
    /// - `pid`: 所属进程 ID
    /// - `tid`: 线程 ID
    fn on_thread_end(&mut self, timestamp: Timestamp, pid: ProcessId, tid: ThreadId);

    /// 处理模块加载事件
    ///
    /// # 参数
    ///
    /// - `timestamp`: 事件时间戳
    /// - `pid`: 加载模块的进程 ID
    /// - `base_address`: 模块基地址
    /// - `module_name`: 模块名称
    fn on_image_load(
        &mut self,
        timestamp: Timestamp,
        pid: ProcessId,
        base_address: Address,
        module_name: &str,
    );

    /// 处理系统调用事件
    ///
    /// # 参数
    ///
    /// - `timestamp`: 事件时间戳
    /// - `pid`: 进程 ID
    /// - `tid`: 线程 ID
    /// - `syscall_id`: 系统调用 ID
    fn on_syscall(
        &mut self,
        timestamp: Timestamp,
        pid: ProcessId,
        tid: ThreadId,
        syscall_id: u32,
    );

    /// 处理原始 ETW 事件记录
    ///
    /// 提供对原始事件数据的访问，用于处理自定义事件类型。
    ///
    /// # 参数
    ///
    /// - `event_record`: Windows ETW 事件记录指针
    ///
    /// # 安全
    ///
    /// 实现者必须确保正确解析事件记录，避免内存安全问题
    fn on_raw_event(&mut self, event_record: *const windows::Win32::System::Diagnostics::Etw::EVENT_RECORD);
}

/// 会话事件回调
///
/// 简化的事件回调 trait，用于不需要完整 EventHandler 实现的场景
pub trait SessionEventCallback: Send + Sync {
    /// 处理事件记录
    ///
    /// # 参数
    ///
    /// - `event_record`: ETW 事件记录
    ///
    /// # 返回
    ///
    /// 如果返回 `true`，表示事件已处理；
    /// 如果返回 `false`，表示事件未被处理
    fn handle_event(
        &self,
        event_record: *const windows::Win32::System::Diagnostics::Etw::EVENT_RECORD,
    ) -> bool;
}

// ============================================================================
// 便利函数和类型
// ============================================================================

/// ETW 会话名称的最大长度
pub const MAX_SESSION_NAME_LENGTH: usize = 1024;

/// 默认缓冲区大小（KB）
pub const DEFAULT_BUFFER_SIZE_KB: u32 = 64;

/// 默认缓冲区数量
pub const DEFAULT_BUFFER_COUNT: u32 = 3;

/// 创建唯一会话名称
///
/// 基于进程 ID 和时间戳生成唯一的 ETW 会话名称
///
/// # 参数
///
/// - `base_name`: 基础名称
///
/// # 返回
///
/// 唯一的会话名称字符串
pub fn generate_unique_session_name(base_name: &str) -> String {
    let pid = std::process::id();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{}_{}_{}", base_name, pid, timestamp)
}

/// 验证会话名称有效性
///
/// ETW 会话名称有长度限制和字符限制
///
/// # 参数
///
/// - `name`: 要验证的会话名称
///
/// # 返回
///
/// 如果名称有效返回 `true`
pub fn is_valid_session_name(name: &str) -> bool {
    if name.is_empty() || name.len() > MAX_SESSION_NAME_LENGTH {
        return false;
    }
    // 检查无效字符
    !name.contains('\0') && !name.contains('\\') && !name.contains('/')
}

/// 采样配置文件事件数据结构
///
/// 表示 CPU 采样事件的核心数据
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SampledProfileEvent {
    /// 事件时间戳（微秒级 Unix 时间戳）
    pub timestamp: Timestamp,
    /// 进程 ID
    pub process_id: ProcessId,
    /// 线程 ID
    pub thread_id: ThreadId,
    /// 指令指针（当前执行的代码地址）
    pub instruction_pointer: Address,
    /// 是否为用户模式
    pub user_mode: bool,
    /// 处理器核心 ID
    pub processor_core: Option<u16>,
    /// 线程优先级
    pub priority: Option<u8>,
}

impl SampledProfileEvent {
    /// 创建新的采样配置文件事件
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
            user_mode: true,
            processor_core: None,
            priority: None,
        }
    }

    /// 设置为内核模式
    pub fn kernel_mode(mut self) -> Self {
        self.user_mode = false;
        self
    }

    /// 设置处理器核心
    pub fn with_processor_core(mut self, core: u16) -> Self {
        self.processor_core = Some(core);
        self
    }

    /// 设置优先级
    pub fn with_priority(mut self, priority: u8) -> Self {
        self.priority = Some(priority);
        self
    }

    /// 转换为通用的 SampleEvent
    pub fn to_sample_event(&self) -> SampleEvent {
        let mut event = SampleEvent::new(
            self.timestamp,
            self.process_id,
            self.thread_id,
            self.instruction_pointer,
        );
        if let Some(core) = self.processor_core {
            event = event.with_processor_core(core);
        }
        if let Some(priority) = self.priority {
            event = event.with_priority(priority);
        }
        event
    }
}

// ============================================================================
// 错误类型转换助手
// ============================================================================

/// 将 Windows 错误代码转换为 Result
///
/// # 参数
///
/// - `result`: Windows API 调用结果
/// - `context`: 错误上下文信息
///
/// # 返回
///
/// 如果 Windows 错误代码非零，返回错误
#[inline]
pub(crate) fn check_win32_error<T>(
    result: windows::core::Result<T>,
    context: &str,
) -> Result<T> {
    result.map_err(|e| {
        let code = e.code().0 as u32;
        crate::error::EtwError::with_code(format!("{}: {:?}", context, e), code).into()
    })
}

/// 安全地转换 UTF-16 字符串
///
/// # 参数
///
/// - `data`: UTF-16 编码的字节切片
///
/// # 返回
///
/// 转换后的字符串，如果无效则返回空字符串
pub(crate) fn safe_utf16_to_string(data: &[u16]) -> String {
    let len = data.iter().position(|&c| c == 0).unwrap_or(data.len());
    String::from_utf16_lossy(&data[..len])
}

// ============================================================================
// 测试模块
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_name_validation() {
        assert!(is_valid_session_name("MySession"));
        assert!(is_valid_session_name("Session_123"));
        assert!(!is_valid_session_name(""));
        assert!(!is_valid_session_name("Session\0Null"));
        assert!(!is_valid_session_name("Session\\Backslash"));
        assert!(!is_valid_session_name("Session/Slash"));
    }

    #[test]
    fn test_generate_unique_session_name() {
        let name1 = generate_unique_session_name("Test");
        let name2 = generate_unique_session_name("Test");
        // 因为包含时间戳，应该不同
        assert_ne!(name1, name2);
        assert!(name1.starts_with("Test_"));
    }

    #[test]
    fn test_sampled_profile_event() {
        let event = SampledProfileEvent::new(1000, 1234, 5678, 0x00401000)
            .kernel_mode()
            .with_processor_core(2)
            .with_priority(8);

        assert_eq!(event.timestamp, 1000);
        assert_eq!(event.process_id, 1234);
        assert_eq!(event.thread_id, 5678);
        assert_eq!(event.instruction_pointer, 0x00401000);
        assert!(!event.user_mode);
        assert_eq!(event.processor_core, Some(2));
        assert_eq!(event.priority, Some(8));

        let sample = event.to_sample_event();
        assert_eq!(sample.timestamp, 1000);
        assert_eq!(sample.process_id, 1234);
    }

    #[test]
    fn test_safe_utf16_conversion() {
        let utf16: Vec<u16> = vec![0x0048, 0x0065, 0x006C, 0x006C, 0x006F, 0x0000]; // "Hello"
        assert_eq!(safe_utf16_to_string(&utf16), "Hello");

        let utf16_no_null: Vec<u16> = vec![0x0057, 0x006F, 0x0072, 0x006C, 0x0064]; // "World"
        assert_eq!(safe_utf16_to_string(&utf16_no_null), "World");
    }
}
