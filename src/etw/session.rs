//! ETW 会话管理模块
//!
//! 提供单个 ETW 会话的完整生命周期管理。

use crate::config::ProfilerConfig;
use crate::error::{EtwError, Result};
use crate::types::ProcessId;
use crate::etw::provider::{KernelProviderFlags, ProviderConfig};
use crate::etw::{is_valid_session_name};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{debug, info, trace, warn};

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
// ETW 会话结构
// ============================================================================

/// ETW 会话管理器
///
/// 管理单个 ETW 会话的生命周期。
pub struct EtwSession {
    /// 会话配置
    config: SessionConfig,
    /// 会话名称（UTF-16 编码，用于 Windows API）
    name_wide: Vec<u16>,
    /// 会话运行状态
    running: Arc<AtomicBool>,
    /// 已启用的提供者列表
    enabled_providers: Vec<windows::core::GUID>,
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

        // 转换为 UTF-16
        let name_wide: Vec<u16> = config.name.encode_utf16().chain(Some(0)).collect();

        info!("Creating ETW session: {}", config.name);

        Ok(Self {
            config,
            name_wide,
            running: Arc::new(AtomicBool::new(false)),
            enabled_providers: Vec::new(),
        })
    }

    /// 启动 ETW 会话
    pub fn start(&mut self) -> Result<()> {
        if self.running.load(Ordering::SeqCst) {
            return Err(EtwError::new("Session is already running").into());
        }

        info!("Starting ETW session: {}", self.config.name);

        // 注意：这里简化实现，实际的 Windows ETW API 调用需要根据具体需求实现
        // 在完整实现中，这里会调用 StartTraceW 等 Windows API
        
        self.running.store(true, Ordering::SeqCst);

        info!(
            "ETW session started successfully: {}",
            self.config.name
        );

        Ok(())
    }

    /// 停止 ETW 会话
    pub fn stop(&mut self) -> Result<()> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(EtwError::new("Session is not running").into());
        }

        info!("Stopping ETW session: {}", self.config.name);

        // 注意：这里简化实现，实际应该调用 ControlTraceW 停止会话
        
        self.enabled_providers.clear();
        self.running.store(false, Ordering::SeqCst);

        info!("ETW session stopped: {}", self.config.name);
        Ok(())
    }

    /// 启用内核提供者
    pub fn enable_kernel_provider(&mut self, flags: KernelProviderFlags) -> Result<()> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(EtwError::new("Session must be started before enabling providers").into());
        }

        info!("Enabling kernel provider with flags: {:?}", flags);

        // 简化实现，实际应该调用 EnableTraceEx2
        debug!("Kernel provider enabled successfully (flags: {})", flags.bits());

        Ok(())
    }

    /// 启用自定义提供者
    pub fn enable_provider(&mut self, provider_config: &ProviderConfig) -> Result<()> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(EtwError::new("Session must be started before enabling providers").into());
        }

        provider_config.validate()?;

        info!("Enabling provider: {:?}", provider_config.guid);
        
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

    #[test]
    fn test_session_lifecycle() {
        let config = ProfilerConfig::default();
        let mut session = EtwSession::new(&config).unwrap();
        
        assert!(!session.is_running());
        
        // 启动会话
        session.start().unwrap();
        assert!(session.is_running());
        
        // 启用提供者
        session.enable_kernel_provider(KernelProviderFlags::PROFILE).unwrap();
        
        // 停止会话
        session.stop().unwrap();
        assert!(!session.is_running());
    }
}
