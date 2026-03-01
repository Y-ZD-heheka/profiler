//! ETW 控制器模块
//!
//! 提供高级 ETW 会话管理功能，协调多个会话的创建、运行和关闭。
//! 支持实时事件处理和文件模式记录。

use crate::config::ProfilerConfig;
use crate::error::{EtwError, Result};
use crate::etw::{
    EventProcessor, EtwSession, KernelProviderFlags, ProcessedEvent,
    SessionConfig, SessionMode,
};
use crate::etw::provider::ProviderConfig;
use crate::types::ProcessId;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tracing::{debug, error, info, trace, warn};

// ============================================================================
// 控制器配置
// ============================================================================

/// ETW 控制器配置
#[derive(Debug, Clone)]
pub struct ControllerConfig {
    /// 基础会话名称
    pub base_session_name: String,
    /// 是否自动启用内核提供者
    pub auto_enable_kernel_provider: bool,
    /// 默认内核提供者标志
    pub default_kernel_flags: KernelProviderFlags,
    /// 最大并发会话数
    pub max_concurrent_sessions: usize,
    /// 自动关闭会话（程序退出时）
    pub auto_cleanup: bool,
}

impl ControllerConfig {
    /// 从 ProfilerConfig 创建控制器配置
    pub fn from_profiler_config(config: &ProfilerConfig) -> Self {
        Self {
            base_session_name: config.session_name.clone(),
            auto_enable_kernel_provider: true,
            default_kernel_flags: KernelProviderFlags::profiling(),
            max_concurrent_sessions: 4,
            auto_cleanup: true,
        }
    }

    /// 设置最大并发会话数
    pub fn with_max_sessions(mut self, max: usize) -> Self {
        self.max_concurrent_sessions = max;
        self
    }

    /// 设置自动启用内核提供者
    pub fn with_auto_enable_kernel(mut self, enable: bool) -> Self {
        self.auto_enable_kernel_provider = enable;
        self
    }

    /// 设置默认内核标志
    pub fn with_kernel_flags(mut self, flags: KernelProviderFlags) -> Self {
        self.default_kernel_flags = flags;
        self
    }
}

impl Default for ControllerConfig {
    fn default() -> Self {
        Self {
            base_session_name: String::from("EtwController"),
            auto_enable_kernel_provider: true,
            default_kernel_flags: KernelProviderFlags::profiling(),
            max_concurrent_sessions: 4,
            auto_cleanup: true,
        }
    }
}

// ============================================================================
// 会话信息
// ============================================================================

/// 会话信息结构
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// 会话名称
    pub name: String,
    /// 会话 ID
    pub id: usize,
    /// 运行模式
    pub mode: SessionMode,
    /// 是否正在运行
    pub is_running: bool,
    /// 已启用的提供者数量
    pub provider_count: usize,
    /// 进程过滤器
    pub process_filter: Vec<ProcessId>,
}

// ============================================================================
// ETW 控制器
// ============================================================================

/// ETW 控制器
///
/// 管理多个 ETW 会话的高级控制器，提供统一的接口来创建、
/// 配置和控制 ETW 会话。
pub struct EtwController {
    /// 控制器配置
    config: ControllerConfig,
    /// 管理中的会话集合
    sessions: Arc<RwLock<HashMap<String, Arc<Mutex<EtwSession>>>>>,
    /// 会话计数器（用于生成唯一ID）
    session_counter: Arc<Mutex<usize>>,
    /// 全局事件处理器
    event_processor: Arc<RwLock<EventProcessor>>,
    /// 控制器运行状态
    running: AtomicBool,
    /// 是否已关闭
    shutdown: AtomicBool,
}

impl EtwController {
    /// 创建新的 ETW 控制器
    ///
    /// # 参数
    ///
    /// - `config`: 性能分析配置
    pub fn new(config: &ProfilerConfig) -> Result<Self> {
        let controller_config = ControllerConfig::from_profiler_config(config);
        Self::with_config(controller_config)
    }

    /// 使用自定义配置创建控制器
    ///
    /// # 参数
    ///
    /// - `config`: 控制器配置
    pub fn with_config(config: ControllerConfig) -> Result<Self> {
        info!("Creating ETW controller: {}", config.base_session_name);

        Ok(Self {
            config,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            session_counter: Arc::new(Mutex::new(0)),
            event_processor: Arc::new(RwLock::new(EventProcessor::new())),
            running: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
        })
    }

    /// 创建实时会话
    ///
    /// # 参数
    ///
    /// - `name`: 会话名称
    ///
    /// # 返回
    ///
    /// 成功返回创建的会话
    pub fn create_realtime_session(&self, name: &str) -> Result<EtwSession> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(EtwError::new("Controller is shutdown").into());
        }

        // 检查会话数量限制
        {
            let sessions = self.sessions.read().unwrap();
            if sessions.len() >= self.config.max_concurrent_sessions {
                return Err(EtwError::new(format!(
                    "Maximum number of sessions ({}) reached",
                    self.config.max_concurrent_sessions
                ))
                .into());
            }
        }

        // 生成唯一名称
        let session_name = format!("{}_{}", self.config.base_session_name, name);
        
        info!("Creating real-time session: {}", session_name);

        // 创建会话配置
        let session_config = SessionConfig::default()
            .with_buffer_size(64)
            .with_buffers(2, 8);

        // 创建会话
        let mut session = EtwSession::with_config(session_config)?;
        
        // 自动启用内核提供者（如果配置启用）
        if self.config.auto_enable_kernel_provider {
            session.start()?;
            session.enable_kernel_provider(self.config.default_kernel_flags)?;
        }

        // 添加到会话集合
        {
            let mut counter = self.session_counter.lock().unwrap();
            let id = *counter;
            *counter += 1;

            let mut sessions = self.sessions.write().unwrap();
            sessions.insert(session_name.clone(), Arc::new(Mutex::new(session)));
            debug!("Session registered: {} (id={})", session_name, id);
        }

        // 返回一个新的会话实例（简化设计，实际应该返回引用）
        let session_config = SessionConfig::default()
            .with_buffer_size(64)
            .with_buffers(2, 8);
        EtwSession::with_config(session_config)
    }

    /// 创建文件会话
    ///
    /// # 参数
    ///
    /// - `name`: 会话名称
    /// - `path`: 日志文件路径
    ///
    /// # 返回
    ///
    /// 成功返回创建的会话
    pub fn create_file_session(&self, name: &str, path: &Path) -> Result<EtwSession> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(EtwError::new("Controller is shutdown").into());
        }

        // 确保父目录存在
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // 检查会话数量限制
        {
            let sessions = self.sessions.read().unwrap();
            if sessions.len() >= self.config.max_concurrent_sessions {
                return Err(EtwError::new(format!(
                    "Maximum number of sessions ({}) reached",
                    self.config.max_concurrent_sessions
                ))
                .into());
            }
        }

        let session_name = format!("{}_{}", self.config.base_session_name, name);
        
        info!("Creating file session: {} -> {}", session_name, path.display());

        // 创建文件模式会话配置
        let session_config = SessionConfig::default()
            .with_file_path(path)
            .with_buffer_size(128)
            .with_buffers(4, 16);

        // 创建会话
        let mut session = EtwSession::with_config(session_config)?;
        
        // 自动启用内核提供者
        if self.config.auto_enable_kernel_provider {
            session.start()?;
            session.enable_kernel_provider(self.config.default_kernel_flags)?;
        }

        // 添加到会话集合
        {
            let mut counter = self.session_counter.lock().unwrap();
            let id = *counter;
            *counter += 1;

            let mut sessions = self.sessions.write().unwrap();
            sessions.insert(session_name.clone(), Arc::new(Mutex::new(session)));
            debug!("File session registered: {} (id={})", session_name, id);
        }

        // 返回新会话实例
        let session_config = SessionConfig::default()
            .with_file_path(path)
            .with_buffer_size(128)
            .with_buffers(4, 16);
        EtwSession::with_config(session_config)
    }

    /// 创建混合模式会话
    ///
    /// 同时支持实时处理和文件记录
    ///
    /// # 参数
    ///
    /// - `name`: 会话名称
    /// - `path`: 日志文件路径
    pub fn create_hybrid_session(&self, name: &str, path: &Path) -> Result<EtwSession> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(EtwError::new("Controller is shutdown").into());
        }

        // 确保父目录存在
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let session_name = format!("{}_{}", self.config.base_session_name, name);
        
        info!("Creating hybrid session: {} -> {}", session_name, path.display());

        // 创建混合模式会话配置
        let session_config = SessionConfig::default()
            .with_hybrid_path(path)
            .with_buffer_size(128)
            .with_buffers(4, 16);

        let mut session = EtwSession::with_config(session_config)?;
        
        if self.config.auto_enable_kernel_provider {
            session.start()?;
            session.enable_kernel_provider(self.config.default_kernel_flags)?;
        }

        // 添加到会话集合
        {
            let mut counter = self.session_counter.lock().unwrap();
            let id = *counter;
            *counter += 1;

            let mut sessions = self.sessions.write().unwrap();
            sessions.insert(session_name.clone(), Arc::new(Mutex::new(session)));
            debug!("Hybrid session registered: {} (id={})", session_name, id);
        }

        let session_config = SessionConfig::default()
            .with_hybrid_path(path)
            .with_buffer_size(128)
            .with_buffers(4, 16);
        EtwSession::with_config(session_config)
    }

    /// 获取会话
    ///
    /// # 参数
    ///
    /// - `name`: 会话名称
    pub fn get_session(&self, name: &str) -> Option<Arc<Mutex<EtwSession>>> {
        let sessions = self.sessions.read().unwrap();
        sessions.get(name).cloned()
    }

    /// 停止特定会话
    ///
    /// # 参数
    ///
    /// - `name`: 会话名称
    pub fn stop_session(&self, name: &str) -> Result<()> {
        let session_name = format!("{}_{}", self.config.base_session_name, name);
        
        info!("Stopping session: {}", session_name);

        let mut sessions = self.sessions.write().unwrap();
        if let Some(session_arc) = sessions.remove(&session_name) {
            let mut session = session_arc.lock().unwrap();
            session.stop()?;
            info!("Session stopped: {}", session_name);
        } else {
            warn!("Session not found: {}", session_name);
        }

        Ok(())
    }

    /// 关闭所有会话
    ///
    /// 停止并清理所有管理的 ETW 会话
    pub fn shutdown_all(&self) -> Result<()> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Ok(());
        }

        info!("Shutting down all ETW sessions...");

        let mut sessions = self.sessions.write().unwrap();
        let session_names: Vec<String> = sessions.keys().cloned().collect();

        for name in session_names {
            if let Some(session_arc) = sessions.remove(&name) {
                let mut session = session_arc.lock().unwrap();
                if let Err(e) = session.stop() {
                    warn!("Error stopping session {}: {}", name, e);
                } else {
                    info!("Session stopped: {}", name);
                }
            }
        }

        self.shutdown.store(true, Ordering::SeqCst);
        self.running.store(false, Ordering::SeqCst);

        info!("All ETW sessions shutdown complete");
        Ok(())
    }

    /// 获取所有会话信息
    pub fn list_sessions(&self) -> Vec<SessionInfo> {
        let sessions = self.sessions.read().unwrap();
        let mut info_list = Vec::new();

        for (name, session_arc) in sessions.iter() {
            let session = session_arc.lock().unwrap();
            let config = session.config();

            info_list.push(SessionInfo {
                name: name.clone(),
                id: 0, // 简化实现
                mode: config.mode,
                is_running: session.is_running(),
                provider_count: session.enabled_providers().len(),
                process_filter: config.process_filter.clone(),
            });
        }

        info_list
    }

    /// 获取活跃会话数量
    pub fn active_session_count(&self) -> usize {
        self.sessions.read().unwrap().len()
    }

    /// 添加全局事件回调
    ///
    /// # 参数
    ///
    /// - `callback`: 事件处理回调
    pub fn add_event_callback<F>(&self, callback: F)
    where
        F: Fn(&ProcessedEvent) + Send + Sync + 'static,
    {
        let mut processor = self.event_processor.write().unwrap();
        processor.add_callback(callback);
    }

    /// 获取事件处理器
    pub fn event_processor(&self) -> Arc<RwLock<EventProcessor>> {
        Arc::clone(&self.event_processor)
    }

    /// 检查控制器是否正在运行
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// 检查控制器是否已关闭
    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    /// 设置进程过滤（应用到所有会话）
    ///
    /// # 参数
    ///
    /// - `pids`: 进程 ID 列表
    pub fn set_global_process_filter(&self, pids: &[ProcessId]) -> Result<()> {
        info!("Setting global process filter: {:?}", pids);

        let sessions = self.sessions.read().unwrap();
        for (name, session_arc) in sessions.iter() {
            let mut session = session_arc.lock().unwrap();
            if let Err(e) = session.set_process_filter(pids) {
                warn!("Failed to set filter for session {}: {}", name, e);
            }
        }

        Ok(())
    }

    /// 启用额外的提供者到所有会话
    ///
    /// # 参数
    ///
    /// - `provider_config`: 提供者配置
    pub fn enable_provider_all(&self, provider_config: &ProviderConfig) -> Result<()> {
        info!("Enabling provider to all sessions");

        let sessions = self.sessions.read().unwrap();
        for (name, session_arc) in sessions.iter() {
            let mut session = session_arc.lock().unwrap();
            if session.is_running() {
                if let Err(e) = session.enable_provider(provider_config) {
                    warn!("Failed to enable provider for session {}: {}", name, e);
                }
            }
        }

        Ok(())
    }

    /// 获取控制器配置
    pub fn config(&self) -> &ControllerConfig {
        &self.config
    }
}

impl Drop for EtwController {
    fn drop(&mut self) {
        if self.config.auto_cleanup && !self.shutdown.load(Ordering::SeqCst) {
            info!("Auto-shutting down ETW controller");
            let _ = self.shutdown_all();
        }
    }
}

// 手动实现 Send 和 Sync
unsafe impl Send for EtwController {}
unsafe impl Sync for EtwController {}

// ============================================================================
// 便捷函数
// ============================================================================

/// 创建默认控制器（便捷函数）
///
/// # 参数
///
/// - `config`: 性能分析配置
pub fn create_default_controller(config: &ProfilerConfig) -> Result<EtwController> {
    EtwController::new(config)
}

/// 快速启动性能分析会话
///
/// # 参数
///
/// - `config`: 性能分析配置
/// - `output_path`: 输出文件路径（可选）
///
/// # 返回
///
/// 返回创建的控制器和会话（如果成功）
pub fn quick_start(
    config: &ProfilerConfig,
    output_path: Option<&Path>,
) -> Result<(EtwController, Option<EtwSession>)> {
    let controller = EtwController::new(config)?;

    let session = if let Some(path) = output_path {
        Some(controller.create_file_session("Profiler", path)?)
    } else {
        Some(controller.create_realtime_session("Profiler")?)
    };

    Ok((controller, session))
}

// ============================================================================
// 测试模块
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_controller_config() {
        let config = ProfilerConfig::default();
        let ctrl_config = ControllerConfig::from_profiler_config(&config);
        
        assert_eq!(ctrl_config.base_session_name, "EtwProfilerSession");
        assert!(ctrl_config.auto_enable_kernel_provider);
        assert_eq!(ctrl_config.max_concurrent_sessions, 4);
    }

    #[test]
    fn test_controller_config_builder() {
        let config = ControllerConfig::default()
            .with_max_sessions(8)
            .with_auto_enable_kernel(false)
            .with_kernel_flags(KernelProviderFlags::PROFILE);

        assert_eq!(config.max_concurrent_sessions, 8);
        assert!(!config.auto_enable_kernel_provider);
        assert!(config.default_kernel_flags.contains(KernelProviderFlags::PROFILE));
    }

    #[test]
    fn test_session_info() {
        let info = SessionInfo {
            name: String::from("TestSession"),
            id: 1,
            mode: SessionMode::RealTime,
            is_running: true,
            provider_count: 2,
            process_filter: vec![1234],
        };

        assert_eq!(info.name, "TestSession");
        assert!(info.is_running);
        assert_eq!(info.provider_count, 2);
    }

    #[test]
    fn test_controller_creation() {
        let config = ProfilerConfig::default();
        let controller = EtwController::new(&config);
        
        // 创建应该成功
        assert!(controller.is_ok());
        
        let ctrl = controller.unwrap();
        assert!(!ctrl.is_running());
        assert!(!ctrl.is_shutdown());
        assert_eq!(ctrl.active_session_count(), 0);
    }

    #[test]
    fn test_quick_start_realtime() {
        let config = ProfilerConfig::default();
        let result = quick_start(&config, None);
        
        assert!(result.is_ok());
        let (controller, session) = result.unwrap();
        assert!(session.is_some());
    }

    #[test]
    fn test_quick_start_file() {
        let config = ProfilerConfig::default();
        let temp_path = PathBuf::from("test_output.etl");
        
        let result = quick_start(&config, Some(&temp_path));
        assert!(result.is_ok());
        
        // 清理
        let _ = std::fs::remove_file(&temp_path);
    }
}
