//! ETW 会话管理模块
//!
//! 提供单个 ETW 会话的完整生命周期管理，包含真正的 Windows ETW API 调用。

use crate::config::ProfilerConfig;
use crate::error::{EtwError, Result};
use crate::types::ProcessId;
use crate::etw::provider::{KernelProviderFlags, ProviderConfig, KERNEL_PROVIDER_GUID};
use crate::etw::{is_valid_session_name, check_win32_error};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{debug, error, info, trace, warn};

use windows::core::{GUID, PCWSTR};
use windows::Win32::System::Diagnostics::Etw::*;
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, ERROR_SUCCESS, WIN32_ERROR};

// ============================================================================
// 会话模式枚举
// ============================================================================

/// ETW 会话运行模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMode {
    /// 实时模式：事件通过回调函数实时处理
    RealTime,
    /// 文件模式：事件写入日志文件
    File,
    /// 混合模式：同时写入文件和实时处理
    Hybrid,
}

impl SessionMode {
    /// 检查是否为实时模式
    pub fn is_realtime(&self) -> bool {
        matches!(self, SessionMode::RealTime | SessionMode::Hybrid)
    }

    /// 检查是否为文件模式
    pub fn is_file(&self) -> bool {
        matches!(self, SessionMode::File | SessionMode::Hybrid)
    }
}

impl Default for SessionMode {
    fn default() -> Self {
        SessionMode::RealTime
    }
}

// ============================================================================
// 会话配置
// ============================================================================

/// ETW 会话配置结构
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// 会话名称
    pub name: String,
    /// 运行模式
    pub mode: SessionMode,
    /// 日志文件路径（文件模式时使用）
    pub log_file_path: Option<PathBuf>,
    /// 缓冲区大小（KB）
    pub buffer_size_kb: u32,
    /// 最小缓冲区数量
    pub min_buffers: u32,
    /// 最大缓冲区数量
    pub max_buffers: u32,
    /// 刷新间隔（秒）
    pub flush_interval_secs: u32,
    /// 进程 ID 过滤器
    pub process_filter: Vec<ProcessId>,
    /// 是否启用堆栈跟踪
    pub enable_stack_trace: bool,
}

impl SessionConfig {
    /// 从 ProfilerConfig 创建会话配置
    pub fn from_profiler_config(config: &ProfilerConfig) -> Self {
        Self {
            name: config.session_name.clone(),
            mode: SessionMode::RealTime,
            log_file_path: None,
            buffer_size_kb: 64,
            min_buffers: 2,
            max_buffers: 8,
            flush_interval_secs: 1,
            process_filter: Vec::new(),
            enable_stack_trace: config.enable_stack_walk,
        }
    }

    /// 设置为文件模式
    pub fn with_file_path(mut self, path: impl AsRef<Path>) -> Self {
        self.log_file_path = Some(path.as_ref().to_path_buf());
        self.mode = SessionMode::File;
        self
    }

    /// 设置为混合模式
    pub fn with_hybrid_path(mut self, path: impl AsRef<Path>) -> Self {
        self.log_file_path = Some(path.as_ref().to_path_buf());
        self.mode = SessionMode::Hybrid;
        self
    }

    /// 设置缓冲区大小
    pub fn with_buffer_size(mut self, size_kb: u32) -> Self {
        self.buffer_size_kb = size_kb;
        self
    }

    /// 设置缓冲区数量
    pub fn with_buffers(mut self, min: u32, max: u32) -> Self {
        self.min_buffers = min;
        self.max_buffers = max.max(min);
        self
    }

    /// 设置刷新间隔
    pub fn with_flush_interval(mut self, secs: u32) -> Self {
        self.flush_interval_secs = secs;
        self
    }

    /// 设置进程过滤器
    pub fn with_process_filter(mut self, pids: &[ProcessId]) -> Self {
        self.process_filter = pids.to_vec();
        self
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            name: String::from("EtwProfilerSession"),
            mode: SessionMode::RealTime,
            log_file_path: None,
            buffer_size_kb: 64,
            min_buffers: 2,
            max_buffers: 8,
            flush_interval_secs: 1,
            process_filter: Vec::new(),
            enable_stack_trace: true,
        }
    }
}

// ============================================================================
// EVENT_TRACE_PROPERTIES 构建器
// ============================================================================

/// 构建 EVENT_TRACE_PROPERTIES 结构
///
/// 这是 Win32 API 需要的复杂结构，包含变长数据
fn build_trace_properties(
    config: &SessionConfig,
    name_wide: &[u16],
) -> Result<Vec<u8>> {
    // 计算需要的额外空间（日志文件名）
    let log_file_len = if let Some(ref path) = config.log_file_path {
        path.as_os_str().encode_wide().count() + 1 // +1 for null terminator
    } else {
        0
    };

    // EVENT_TRACE_PROPERTIES 基础大小 + 会话名称空间 + 日志文件路径空间
    let name_len = name_wide.len();
    let total_size = std::mem::size_of::<EVENT_TRACE_PROPERTIES>() 
        + (name_len * 2) 
        + (log_file_len * 2);

    let mut buffer = vec![0u8; total_size];
    
    // 获取可变引用
    let props = unsafe {
        &mut *(buffer.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES)
    };

    // 设置 Wnode
    props.Wnode.BufferSize = total_size as u32;
    props.Wnode.Guid = GUID::new().unwrap_or(GUID::default());
    props.Wnode.ClientContext = 1; // QPC clock resolution
    props.Wnode.Flags = WNODE_FLAG_TRACED_GUID;

    // 设置缓冲区配置
    props.BufferSize = config.buffer_size_kb;
    props.MinimumBuffers = config.min_buffers;
    props.MaximumBuffers = config.max_buffers;
    props.FlushTimer = config.flush_interval_secs;
    props.LogFileMode = if config.mode.is_realtime() {
        EVENT_TRACE_REAL_TIME_MODE
    } else {
        EVENT_TRACE_FILE_MODE_SEQUENTIAL
    };

    // 启用堆栈跟踪（如果配置要求）
    if config.enable_stack_trace {
        props.LogFileMode |= EVENT_TRACE_SYSTEM_LOGGER_MODE;
    }

    // 设置 LoggerNameOffset 和 LogFileNameOffset
    let base_size = std::mem::size_of::<EVENT_TRACE_PROPERTIES>();
    props.LoggerNameOffset = base_size as u32;
    
    if log_file_len > 0 {
        props.LogFileNameOffset = (base_size + name_len * 2) as u32;
    }

    // 复制会话名称
    unsafe {
        let name_ptr = buffer.as_mut_ptr().add(base_size) as *mut u16;
        std::ptr::copy_nonoverlapping(name_wide.as_ptr(), name_ptr, name_len);

        // 复制日志文件路径（如果有）
        if let Some(ref path) = config.log_file_path {
            let file_wide: Vec<u16> = path.as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect();
            let file_ptr = buffer.as_mut_ptr().add(base_size + name_len * 2) as *mut u16;
            std::ptr::copy_nonoverlapping(file_wide.as_ptr(), file_ptr, file_wide.len());
        }
    }

    Ok(buffer)
}

// ============================================================================
// ETW 会话结构
// ============================================================================

/// ETW 会话管理器
///
/// 管理单个 ETW 会话的生命周期，包含真正的 Windows ETW API 调用。
pub struct EtwSession {
    /// 会话配置
    config: SessionConfig,
    /// 会话名称（UTF-16 编码，用于 Windows API）
    name_wide: Vec<u16>,
    /// 会话运行状态
    running: Arc<AtomicBool>,
    /// 已启用的提供者列表
    enabled_providers: Vec<windows::core::GUID>,
    /// ETW 会话句柄
    session_handle: CONTROLTRACE_HANDLE,
    /// 会话注册句柄（用于停止）
    registration_handle: u64,
    /// 跟踪属性缓冲区（需要保持存活）
    _properties_buffer: Option<Vec<u8>>,
}

// 手动实现 Send
unsafe impl Send for EtwSession {}

impl EtwSession {
    /// 创建新的 ETW 会话
    pub fn new(config: &ProfilerConfig) -> Result<Self> {
        let session_config = SessionConfig::from_profiler_config(config);
        Self::with_config(session_config)
    }

    /// 使用自定义配置创建会话
    pub fn with_config(config: SessionConfig) -> Result<Self> {
        // 验证会话名称
        if !is_valid_session_name(&config.name) {
            return Err(EtwError::new(format!(
                "Invalid session name: {}",
                config.name
            ))
            .into());
        }

        // 验证文件路径（文件模式时）
        if config.mode.is_file() && config.log_file_path.is_none() {
            return Err(EtwError::new("File mode requires log file path").into());
        }

        // 转换为 UTF-16（包含 null terminator）
        let name_wide: Vec<u16> = config.name.encode_utf16().chain(Some(0)).collect();

        info!("Creating ETW session: {}", config.name);

        Ok(Self {
            config,
            name_wide,
            running: Arc::new(AtomicBool::new(false)),
            enabled_providers: Vec::new(),
            session_handle: CONTROLTRACE_HANDLE { Value: 0 },
            registration_handle: 0,
            _properties_buffer: None,
        })
    }

    /// 启动 ETW 会话
    ///
    /// 调用 StartTraceW 创建 ETW 会话
    pub fn start(&mut self) -> Result<()> {
        if self.running.load(Ordering::SeqCst) {
            return Err(EtwError::new("Session is already running").into());
        }

        info!("Starting ETW session: {}", self.config.name);

        // 构建跟踪属性
        let mut properties_buffer = build_trace_properties(&self.config, &self.name_wide)?;
        
        // 调用 StartTraceW
        let session_name_pcwstr = PCWSTR(self.name_wide.as_ptr());
        let mut session_handle: CONTROLTRACE_HANDLE = CONTROLTRACE_HANDLE { Value: 0 };

        let result = unsafe {
            StartTraceW(
                &mut session_handle,
                session_name_pcwstr,
                properties_buffer.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES,
            )
        };

        if result == ERROR_SUCCESS || result == ERROR_ALREADY_EXISTS {
            if result == ERROR_ALREADY_EXISTS {
                warn!("ETW session already exists, stopping and restarting...");
                // 停止已存在的会话
                let _ = self.stop_existing_session();
                // 重试启动
                return self.start();
            }

            self.session_handle = session_handle;
            self._properties_buffer = Some(properties_buffer);
            self.running.store(true, Ordering::SeqCst);

            info!(
                "ETW session started successfully: {} (handle={})",
                self.config.name, self.session_handle.Value
            );

            Ok(())
        } else {
            let error_msg = match result.0 {
                5 => "Access denied. ETW profiling requires administrator privileges. Please run as administrator.".to_string(),
                87 => "Invalid parameter. Check ETW session configuration.".to_string(),
                122 => "Insufficient buffer size for ETW properties.".to_string(),
                183 => "ETW session already exists with different configuration.".to_string(),
                _ => format!("Failed to start ETW session: {:?}", result),
            };
            Err(EtwError::with_code(error_msg, result.0).into())
        }
    }

    /// 停止已存在的会话
    fn stop_existing_session(&self) -> Result<()> {
        let name_pcwstr = PCWSTR(self.name_wide.as_ptr());
        
        // 创建一个临时属性结构来获取会话信息
        let mut temp_props = EVENT_TRACE_PROPERTIES {
            Wnode: WNODE_HEADER {
                BufferSize: std::mem::size_of::<EVENT_TRACE_PROPERTIES>() as u32,
                ..Default::default()
            },
            ..Default::default()
        };

        unsafe {
            let _ = ControlTraceW(
                CONTROLTRACE_HANDLE { Value: 0 },
                name_pcwstr,
                &mut temp_props,
                EVENT_TRACE_CONTROL_STOP,
            );
        }

        Ok(())
    }

    /// 停止 ETW 会话
    ///
    /// 调用 ControlTraceW 停止会话
    pub fn stop(&mut self) -> Result<()> {
        if !self.running.load(Ordering::SeqCst) {
            return Ok(()); // 已停止，不返回错误
        }

        info!("Stopping ETW session: {}", self.config.name);

        if self.session_handle.Value != 0 {
            let name_pcwstr = PCWSTR(self.name_wide.as_ptr());
            
            // 创建属性结构用于 ControlTraceW
            let mut temp_props = EVENT_TRACE_PROPERTIES {
                Wnode: WNODE_HEADER {
                    BufferSize: std::mem::size_of::<EVENT_TRACE_PROPERTIES>() as u32,
                    ..Default::default()
                },
                ..Default::default()
            };

            let result = unsafe {
                ControlTraceW(
                    self.session_handle,
                    name_pcwstr,
                    &mut temp_props,
                    EVENT_TRACE_CONTROL_STOP,
                )
            };

            if result != ERROR_SUCCESS {
                warn!("ControlTraceW stop returned: {:?}", result);
            }
        }

        self.enabled_providers.clear();
        self.session_handle = CONTROLTRACE_HANDLE { Value: 0 };
        self.registration_handle = 0;
        self._properties_buffer = None;
        self.running.store(false, Ordering::SeqCst);

        info!("ETW session stopped: {}", self.config.name);
        Ok(())
    }

    /// 启用内核提供者
    ///
    /// 调用 EnableTraceEx2 启用内核 ETW 提供者
    pub fn enable_kernel_provider(&mut self, flags: KernelProviderFlags) -> Result<()> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(EtwError::new("Session must be started before enabling providers").into());
        }

        if self.session_handle.Value == 0 {
            return Err(EtwError::new("Invalid session handle").into());
        }

        info!("Enabling kernel provider with flags: 0x{:08X}", flags.bits());

        // 使用 EnableTraceEx2 启用内核提供者
        let result = unsafe {
            EnableTraceEx2(
                self.session_handle,
                &KERNEL_PROVIDER_GUID,
                EVENT_CONTROL_CODE_ENABLE_PROVIDER.0,
                TRACE_LEVEL_INFORMATION as u8,
                flags.bits() as u64,
                0,
                0,
                None,
            )
        };

        if result != ERROR_SUCCESS {
            return Err(EtwError::with_code(
                format!("Failed to enable kernel provider: {:?}", result),
                result.0,
            )
            .into());
        }

        self.enabled_providers.push(KERNEL_PROVIDER_GUID);
        
        info!(
            "Kernel provider enabled successfully (flags: 0x{:08X})",
            flags.bits()
        );

        Ok(())
    }

    /// 启用自定义提供者
    pub fn enable_provider(&mut self, provider_config: &ProviderConfig) -> Result<()> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(EtwError::new("Session must be started before enabling providers").into());
        }

        if self.session_handle.Value == 0 {
            return Err(EtwError::new("Invalid session handle").into());
        }

        provider_config.validate()?;

        info!("Enabling provider: {:?}", provider_config.guid);

        let result = unsafe {
            EnableTraceEx2(
                self.session_handle,
                &provider_config.guid,
                EVENT_CONTROL_CODE_ENABLE_PROVIDER.0,
                provider_config.level,
                provider_config.match_any_keyword,
                provider_config.match_all_keyword,
                provider_config.enable_flags as u32,
                None,
            )
        };

        if result != ERROR_SUCCESS {
            return Err(EtwError::with_code(
                format!("Failed to enable provider: {:?}", result),
                result.0,
            )
            .into());
        }

        self.enabled_providers.push(provider_config.guid);
        debug!("Provider enabled successfully");

        Ok(())
    }

    /// 设置进程过滤
    pub fn set_process_filter(&mut self, pids: &[ProcessId]) -> Result<()> {
        if self.running.load(Ordering::SeqCst) {
            warn!("Process filter applied to running session may not take effect immediately");
        }

        self.config.process_filter = pids.to_vec();
        info!("Process filter set for PIDs: {:?}", pids);
        Ok(())
    }

    /// 检查会话是否正在运行
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// 获取会话名称
    pub fn name(&self) -> &str {
        &self.config.name
    }

    /// 获取会话配置
    pub fn config(&self) -> &SessionConfig {
        &self.config
    }

    /// 获取已启用的提供者列表
    pub fn enabled_providers(&self) -> &[windows::core::GUID] {
        &self.enabled_providers
    }

    /// 获取会话句柄
    pub fn session_handle(&self) -> CONTROLTRACE_HANDLE {
        self.session_handle
    }
}

impl Drop for EtwSession {
    fn drop(&mut self) {
        if self.running.load(Ordering::SeqCst) {
            info!("Auto-stopping ETW session on drop: {}", self.config.name);
            let _ = self.stop();
        }
    }
}

// ============================================================================
// 测试模块
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_mode() {
        assert!(SessionMode::RealTime.is_realtime());
        assert!(!SessionMode::RealTime.is_file());
        
        assert!(!SessionMode::File.is_realtime());
        assert!(SessionMode::File.is_file());
        
        assert!(SessionMode::Hybrid.is_realtime());
        assert!(SessionMode::Hybrid.is_file());
    }

    #[test]
    fn test_session_config_builder() {
        let config = SessionConfig::default()
            .with_file_path("test.etl")
            .with_buffer_size(128)
            .with_buffers(4, 16)
            .with_process_filter(&[1234, 5678]);

        assert!(config.log_file_path.is_some());
        assert_eq!(config.buffer_size_kb, 128);
        assert_eq!(config.min_buffers, 4);
        assert_eq!(config.max_buffers, 16);
        assert_eq!(config.process_filter, vec![1234, 5678]);
    }

    #[test]
    fn test_invalid_session_name() {
        let config = ProfilerConfig::default();
        // 尝试创建带有无效字符的会话
        let mut session_config = SessionConfig::from_profiler_config(&config);
        session_config.name = String::from("Test\0Null");
        
        let result = EtwSession::with_config(session_config);
        assert!(result.is_err());
    }
}
