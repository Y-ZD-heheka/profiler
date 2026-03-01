//! 会话状态管理模块
//!
//! 提供性能分析会话的状态管理和生命周期控制。

use crate::analysis::AnalysisResult;
use crate::error::{ProfilerError, Result};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// 会话状态枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// 正在初始化
    Initializing,
    /// 正在运行
    Running { process_id: Option<u32> },
    /// 已暂停
    Paused,
    /// 正在停止
    Stopping,
    /// 已完成
    Completed,
    /// 失败
    Failed,
}

impl std::fmt::Display for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionState::Initializing => write!(f, "Initializing"),
            SessionState::Running { process_id: None } => write!(f, "Running"),
            SessionState::Running { process_id: Some(pid) } => write!(f, "Running (PID: {})", pid),
            SessionState::Paused => write!(f, "Paused"),
            SessionState::Stopping => write!(f, "Stopping"),
            SessionState::Completed => write!(f, "Completed"),
            SessionState::Failed => write!(f, "Failed"),
        }
    }
}

/// 性能分析会话
///
/// 管理单次性能分析会话的完整生命周期，包括状态跟踪、统计信息收集和结果存储。
pub struct ProfilerSession {
    /// 会话唯一标识符
    session_id: Uuid,
    /// 会话开始时间
    start_time: Instant,
    /// 会话状态
    state: Arc<RwLock<SessionState>>,
    /// 目标进程ID
    target_process_id: Arc<RwLock<Option<u32>>>,
    /// 分析结果
    result: Arc<RwLock<Option<AnalysisResult>>>,
    /// 错误信息
    error: Arc<RwLock<Option<ProfilerError>>>,
    /// 是否被取消
    cancelled: Arc<AtomicBool>,
    /// 样本计数
    sample_count: Arc<AtomicU64>,
    /// 解析的堆栈数
    resolved_stacks: Arc<AtomicU64>,
    /// 跟踪的线程数
    thread_count: Arc<RwLock<usize>>,
}

impl ProfilerSession {
    /// 创建新的性能分析会话
    pub fn new() -> Self {
        let session_id = Uuid::new_v4();
        info!("Creating new profiler session: {}", session_id);

        Self {
            session_id,
            start_time: Instant::now(),
            state: Arc::new(RwLock::new(SessionState::Initializing)),
            target_process_id: Arc::new(RwLock::new(None)),
            result: Arc::new(RwLock::new(None)),
            error: Arc::new(RwLock::new(None)),
            cancelled: Arc::new(AtomicBool::new(false)),
            sample_count: Arc::new(AtomicU64::new(0)),
            resolved_stacks: Arc::new(AtomicU64::new(0)),
            thread_count: Arc::new(RwLock::new(0)),
        }
    }

    /// 获取会话ID
    pub fn session_id(&self) -> Uuid {
        self.session_id
    }

    /// 获取开始时间
    pub fn start_time(&self) -> Instant {
        self.start_time
    }

    /// 获取已用时间
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// 获取当前状态
    pub fn state(&self) -> SessionState {
        self.state.read().map(|s| *s).unwrap_or(SessionState::Failed)
    }

    /// 设置状态
    pub fn set_state(&self, new_state: SessionState) {
        if let Ok(mut state) = self.state.write() {
            info!("Session {} state: {:?} -> {:?}", self.session_id, *state, new_state);
            *state = new_state;
        }
    }

    /// 标记为正在运行
    pub fn mark_running(&self, process_id: Option<u32>) {
        self.set_state(SessionState::Running { process_id });
        if let Ok(mut pid) = self.target_process_id.write() {
            *pid = process_id;
        }
    }

    /// 标记为已暂停
    pub fn mark_paused(&self) {
        self.set_state(SessionState::Paused);
    }

    /// 标记为正在停止
    pub fn mark_stopping(&self) {
        self.set_state(SessionState::Stopping);
    }

    /// 标记为已完成
    pub fn mark_completed(&self) {
        self.set_state(SessionState::Completed);
    }

    /// 标记为失败
    pub fn mark_failed(&self, error: ProfilerError) {
        self.set_state(SessionState::Failed);
        if let Ok(mut err) = self.error.write() {
            *err = Some(error);
        }
    }

    /// 检查是否正在运行
    pub fn is_running(&self) -> bool {
        matches!(self.state(), SessionState::Running { .. })
    }

    /// 检查是否已完成
    pub fn is_completed(&self) -> bool {
        self.state() == SessionState::Completed
    }

    /// 检查是否已失败
    pub fn is_failed(&self) -> bool {
        self.state() == SessionState::Failed
    }

    /// 检查是否被取消
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// 请求取消
    pub fn cancel(&self) {
        info!("Session {} cancellation requested", self.session_id);
        self.cancelled.store(true, Ordering::SeqCst);
        self.mark_stopping();
    }

    /// 获取目标进程ID
    pub fn target_process_id(&self) -> Option<u32> {
        self.target_process_id.read().ok().and_then(|p| *p)
    }

    /// 设置分析结果
    pub fn set_result(&self, result: AnalysisResult) {
        if let Ok(mut res) = self.result.write() {
            *res = Some(result);
        }
        self.mark_completed();
    }

    /// 获取分析结果
    pub fn get_result(&self) -> Option<AnalysisResult> {
        self.result.read().ok().and_then(|r| r.clone())
    }

    /// 获取错误信息
    pub fn get_error(&self) -> Option<String> {
        self.error.read().ok().and_then(|e| e.as_ref().map(|err| err.to_string()))
    }

    /// 增加样本计数
    pub fn increment_samples(&self, count: u64) {
        self.sample_count.fetch_add(count, Ordering::Relaxed);
    }

    /// 获取样本计数
    pub fn sample_count(&self) -> u64 {
        self.sample_count.load(Ordering::Relaxed)
    }

    /// 增加解析堆栈计数
    pub fn increment_resolved(&self, count: u64) {
        self.resolved_stacks.fetch_add(count, Ordering::Relaxed);
    }

    /// 获取解析堆栈计数
    pub fn resolved_count(&self) -> u64 {
        self.resolved_stacks.load(Ordering::Relaxed)
    }

    /// 设置线程数
    pub fn set_thread_count(&self, count: usize) {
        if let Ok(mut threads) = self.thread_count.write() {
            *threads = count;
        }
    }

    /// 获取线程数
    pub fn thread_count(&self) -> usize {
        self.thread_count.read().map(|t| *t).unwrap_or(0)
    }

    /// 获取会话统计信息
    pub fn stats(&self) -> SessionStats {
        SessionStats {
            session_id: self.session_id,
            elapsed: self.elapsed(),
            state: self.state(),
            sample_count: self.sample_count(),
            resolved_stacks: self.resolved_count(),
            thread_count: self.thread_count(),
            target_process_id: self.target_process_id(),
        }
    }

    /// 等待会话完成
    pub fn wait_for_completion(&self, timeout: Option<Duration>) -> Result<()> {
        let start = Instant::now();
        
        loop {
            let state = self.state();
            
            match state {
                SessionState::Completed => return Ok(()),
                SessionState::Failed => {
                    let err_msg = self.get_error()
                        .unwrap_or_else(|| "Unknown error".to_string());
                    return Err(ProfilerError::Generic(err_msg));
                }
                _ => {
                    // 检查超时
                    if let Some(timeout) = timeout {
                        if start.elapsed() >= timeout {
                            return Err(ProfilerError::Generic("Wait timeout".to_string()));
                        }
                    }
                    
                    // 检查是否被取消
                    if self.is_cancelled() {
                        return Ok(());
                    }
                    
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }
}

impl Default for ProfilerSession {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for ProfilerSession {
    fn clone(&self) -> Self {
        Self {
            session_id: self.session_id,
            start_time: self.start_time,
            state: Arc::clone(&self.state),
            target_process_id: Arc::clone(&self.target_process_id),
            result: Arc::clone(&self.result),
            error: Arc::clone(&self.error),
            cancelled: Arc::clone(&self.cancelled),
            sample_count: Arc::clone(&self.sample_count),
            resolved_stacks: Arc::clone(&self.resolved_stacks),
            thread_count: Arc::clone(&self.thread_count),
        }
    }
}

/// 会话统计信息
#[derive(Debug, Clone)]
pub struct SessionStats {
    /// 会话ID
    pub session_id: Uuid,
    /// 已用时间
    pub elapsed: Duration,
    /// 当前状态
    pub state: SessionState,
    /// 样本计数
    pub sample_count: u64,
    /// 解析的堆栈数
    pub resolved_stacks: u64,
    /// 线程数
    pub thread_count: usize,
    /// 目标进程ID
    pub target_process_id: Option<u32>,
}

impl SessionStats {
    /// 格式化已用时间为人类可读格式
    pub fn format_elapsed(&self) -> String {
        let secs = self.elapsed.as_secs();
        if secs < 60 {
            format!("{}s", secs)
        } else if secs < 3600 {
            format!("{}m {}s", secs / 60, secs % 60)
        } else {
            format!("{}h {}m {}s", secs / 3600, (secs % 3600) / 60, secs % 60)
        }
    }
}

/// 会话管理器
///
/// 管理多个性能分析会话，提供会话创建、查询和清理功能。
pub struct SessionManager {
    /// 活跃会话映射
    sessions: Arc<RwLock<std::collections::HashMap<Uuid, ProfilerSession>>>,
    /// 默认超时时间
    default_timeout: Duration,
}

impl SessionManager {
    /// 创建新的会话管理器
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(std::collections::HashMap::new())),
            default_timeout: Duration::from_secs(300),
        }
    }

    /// 创建新会话
    pub fn create_session(&self) -> ProfilerSession {
        let session = ProfilerSession::new();
        let id = session.session_id();
        
        if let Ok(mut sessions) = self.sessions.write() {
            sessions.insert(id, session.clone());
        }
        
        info!("Created session {}, total sessions: {}", 
            id, 
            self.session_count()
        );
        
        session
    }

    /// 获取会话
    pub fn get_session(&self, id: Uuid) -> Option<ProfilerSession> {
        self.sessions.read().ok().and_then(|s| s.get(&id).cloned())
    }

    /// 移除会话
    pub fn remove_session(&self, id: Uuid) -> Option<ProfilerSession> {
        let session = self.sessions.write().ok().and_then(|mut s| s.remove(&id));
        if session.is_some() {
            debug!("Removed session {}, remaining: {}", id, self.session_count());
        }
        session
    }

    /// 获取所有会话
    pub fn get_all_sessions(&self) -> Vec<ProfilerSession> {
        self.sessions
            .read()
            .map(|s| s.values().cloned().collect())
            .unwrap_or_default()
    }

    /// 获取活跃会话数
    pub fn session_count(&self) -> usize {
        self.sessions.read().map(|s| s.len()).unwrap_or(0)
    }

    /// 清理已完成的会话
    pub fn cleanup_completed(&self) -> usize {
        let to_remove: Vec<Uuid> = self
            .sessions
            .read()
            .map(|s| {
                s.values()
                    .filter(|session| session.is_completed() || session.is_failed())
                    .map(|session| session.session_id())
                    .collect()
            })
            .unwrap_or_default();

        let count = to_remove.len();
        for id in to_remove {
            self.remove_session(id);
        }

        if count > 0 {
            info!("Cleaned up {} completed sessions", count);
        }
        
        count
    }

    /// 取消所有会话
    pub fn cancel_all(&self) {
        let sessions = self.get_all_sessions();
        for session in sessions {
            session.cancel();
        }
        info!("Cancelled all {} sessions", self.session_count());
    }

    /// 设置默认超时
    pub fn set_default_timeout(&mut self, timeout: Duration) {
        self.default_timeout = timeout;
    }

    /// 获取默认超时
    pub fn default_timeout(&self) -> Duration {
        self.default_timeout
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_creation() {
        let session = ProfilerSession::new();
        assert_eq!(session.state(), SessionState::Initializing);
        assert!(!session.is_running());
        assert!(!session.is_completed());
    }

    #[test]
    fn test_session_state_transitions() {
        let session = ProfilerSession::new();
        
        session.mark_running(Some(1234));
        assert!(session.is_running());
        assert_eq!(session.target_process_id(), Some(1234));
        
        session.mark_paused();
        assert!(!session.is_running());
        
        session.mark_completed();
        assert!(session.is_completed());
    }

    #[test]
    fn test_session_counters() {
        let session = ProfilerSession::new();
        
        session.increment_samples(10);
        assert_eq!(session.sample_count(), 10);
        
        session.increment_resolved(5);
        assert_eq!(session.resolved_count(), 5);
        
        session.set_thread_count(3);
        assert_eq!(session.thread_count(), 3);
    }

    #[test]
    fn test_session_cancel() {
        let session = ProfilerSession::new();
        
        assert!(!session.is_cancelled());
        session.cancel();
        assert!(session.is_cancelled());
    }

    #[test]
    fn test_session_stats() {
        let session = ProfilerSession::new();
        session.increment_samples(100);
        session.set_thread_count(5);
        
        let stats = session.stats();
        assert_eq!(stats.sample_count, 100);
        assert_eq!(stats.thread_count, 5);
        assert_eq!(stats.session_id, session.session_id());
    }

    #[test]
    fn test_session_manager() {
        let manager = SessionManager::new();
        
        let session1 = manager.create_session();
        let session2 = manager.create_session();
        
        assert_eq!(manager.session_count(), 2);
        
        let retrieved = manager.get_session(session1.session_id());
        assert!(retrieved.is_some());
        
        manager.remove_session(session2.session_id());
        assert_eq!(manager.session_count(), 1);
    }

    #[test]
    fn test_session_state_display() {
        assert_eq!(format!("{}", SessionState::Initializing), "Initializing");
        assert_eq!(format!("{}", SessionState::Running { process_id: None }), "Running");
        assert_eq!(format!("{}", SessionState::Running { process_id: Some(1234) }), "Running (PID: 1234)");
        assert_eq!(format!("{}", SessionState::Completed), "Completed");
    }
}
