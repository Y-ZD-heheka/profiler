//! 性能分析器协调器模块
//!
//! 协调所有模块的工作，提供统一的性能分析接口。
//! 管理ETW会话、符号解析、堆栈收集和数据分析的整个流程。

use crate::analysis::{AggregatorConfig, AnalysisResult, StatsAggregator};
use crate::config::ProfilerConfig;
use crate::error::{ProcessError, ProfilerError, Result};
use crate::etw::{EtwController, EtwSession, EventProcessor, KernelProviderFlags, ProcessedEvent, SampledProfileEvent};
use crate::report::{CsvReportGenerator, ReportConfig, ReportGenerator};
use crate::session::{ProfilerSession, SessionState};
use crate::stackwalker::{CollectedStack, StackManager, StackManagerConfig};
use crate::symbols::SymbolManager;
use crate::types::{ProcessStats, SampleEvent};

use std::path::Path;
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};

/// 性能分析器进度信息
#[derive(Debug, Clone)]
pub struct ProfilerProgress {
    /// 已收集的样本数
    pub samples_collected: u64,
    /// 已解析的堆栈数
    pub stacks_resolved: u64,
    /// 跟踪的线程数
    pub threads_tracked: usize,
    /// 已用时间
    pub elapsed_time: Duration,
    /// 当前状态
    pub status: String,
}

impl Default for ProfilerProgress {
    fn default() -> Self {
        Self {
            samples_collected: 0,
            stacks_resolved: 0,
            threads_tracked: 0,
            elapsed_time: Duration::ZERO,
            status: String::from("Idle"),
        }
    }
}

/// 性能分析器
///
/// 协调所有模块，提供统一的性能分析接口。
/// 支持两种检测模式：启动新进程（Launch）和附加到现有进程（Attach）。
pub struct Profiler {
    /// 配置
    config: ProfilerConfig,
    /// 当前会话
    session: ProfilerSession,
    /// ETW 控制器
    etw_controller: Option<EtwController>,
    /// ETW 会话
    etw_session: Option<EtwSession>,
    /// 符号管理器
    symbol_manager: Arc<SymbolManager>,
    /// 堆栈管理器
    stack_manager: StackManager,
    /// 统计聚合器
    aggregator: Arc<Mutex<StatsAggregator>>,
    /// 是否正在运行
    running: Arc<AtomicBool>,
    /// 关闭信号发送器
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl Profiler {
    /// 创建新的性能分析器
    ///
    /// # 参数
    /// - `config`: 性能分析配置
    pub fn new(config: ProfilerConfig) -> Result<Self> {
        info!("Creating new profiler with session name: {}", config.session_name);

        // 创建会话
        let session = ProfilerSession::new();
        
        // 创建符号管理器
        let symbol_manager = Arc::new(SymbolManager::new());
        
        // 添加配置的符号路径
        for path in &config.symbol_paths {
            symbol_manager.add_symbol_path(path.clone());
        }

        // 创建堆栈管理器配置
        let stack_config = StackManagerConfig::default()
            .with_max_depth(config.max_stack_depth as usize)
            .with_kernel_stack(true)
            .with_skip_system_modules(!config.include_system_calls);

        let stack_manager = StackManager::new(stack_config);

        // 初始化堆栈管理器
        stack_manager.initialize(Arc::clone(&symbol_manager))?;

        // 创建聚合器
        let aggregator_config = AggregatorConfig::default()
            .with_sample_interval_ms(config.sample_interval_ms)
            .with_exclude_system_functions(!config.include_system_calls);
        let aggregator = Arc::new(Mutex::new(StatsAggregator::new(aggregator_config)));

        Ok(Self {
            config,
            session,
            etw_controller: None,
            etw_session: None,
            symbol_manager,
            stack_manager,
            aggregator,
            running: Arc::new(AtomicBool::new(false)),
            shutdown_tx: None,
        })
    }

    /// 运行性能分析
    ///
    /// 启动ETW会话，收集采样数据，直到会话结束或被取消。
    pub async fn run(&mut self) -> Result<AnalysisResult> {
        info!("Starting profiler run");
        
        self.session.mark_running(None);
        self.running.store(true, Ordering::SeqCst);

        // 创建ETW控制器并设置事件回调
        self.setup_etw_controller()?;

        // 创建实时会话
        let session = self.etw_controller.as_ref().unwrap()
            .create_realtime_session(&self.config.session_name)?;
        self.etw_session = Some(session);

        // 设置关闭通道
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx);

        // 获取目标进程ID
        let target_pid = self.config.target_process.as_ref()
            .and_then(|t| t.as_pid())
            .unwrap_or(0);

        // 启动采样收集任务
        let sample_interval = Duration::from_millis(self.config.sample_interval_ms as u64);
        let duration = if self.config.duration_secs > 0 {
            Some(Duration::from_secs(self.config.duration_secs))
        } else {
            None
        };

        let running = Arc::clone(&self.running);
        let session_clone = self.session.clone();
        let aggregator = Arc::clone(&self.aggregator);
        let stack_manager = Arc::clone(&self.symbol_manager);
        let config = self.config.clone();

        // 在单独任务中运行收集循环
        let collector_handle = tokio::spawn(async move {
            let start_time = Instant::now();
            let mut last_sample_count: u64 = 0;
            let mut sample_counter: u64 = 0;

            // 初始化堆栈收集
            let stack_mgr_config = StackManagerConfig::default()
                .with_max_depth(config.max_stack_depth as usize)
                .with_kernel_stack(config.enable_stack_walk)
                .with_skip_system_modules(!config.include_system_calls);
            
            let stack_mgr = StackManager::new(stack_mgr_config);
            if let Err(e) = stack_mgr.initialize(stack_manager) {
                warn!("Failed to initialize stack manager: {}", e);
            }

            while running.load(Ordering::SeqCst) {
                // 检查持续时间
                if let Some(dur) = duration {
                    if start_time.elapsed() >= dur {
                        info!("Duration limit reached");
                        break;
                    }
                }

                // 生成模拟采样事件（用于测试数据流）
                // 在实际实现中，这里应该从 ETW 事件流读取
                if target_pid != 0 && stack_mgr.is_initialized() {
                    // 尝试收集目标进程的样本
                    let sample = SampledProfileEvent::new(
                        start_time.elapsed().as_micros() as u64,
                        target_pid,
                        0, // 线程ID会在实际收集中获取
                        0x00400000 + sample_counter, // 模拟指令指针
                    );

                    match stack_mgr.on_sample(&sample) {
                        Ok(Some(stack)) => {
                            if let Ok(mut agg) = aggregator.lock() {
                                if let Err(e) = agg.process_stack(&stack) {
                                    trace!("Failed to process stack: {}", e);
                                } else {
                                    sample_counter += 1;
                                }
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            trace!("Stack collection error: {}", e);
                        }
                    }
                }

                // 从聚合器获取当前统计
                if let Ok(agg) = aggregator.lock() {
                    let current_samples = agg.total_samples();
                    if current_samples > last_sample_count {
                        session_clone.increment_samples(current_samples - last_sample_count);
                        last_sample_count = current_samples;
                    }
                    session_clone.set_thread_count(agg.thread_count());
                    
                    // 更新状态
                    if current_samples > 0 && current_samples % 100 == 0 {
                        debug!("Collected {} samples, {} threads",
                            current_samples, agg.thread_count());
                    }
                }

                // 检查关闭信号
                if shutdown_rx.try_recv().is_ok() {
                    info!("Shutdown signal received");
                    break;
                }

                tokio::time::sleep(sample_interval).await;
            }

            let _ = stack_mgr.shutdown();
            last_sample_count
        });

        // 等待收集完成或超时
        let timeout_duration = duration.map(|d| d + Duration::from_secs(5))
            .unwrap_or_else(|| Duration::from_secs(300));
        
        let result = tokio::time::timeout(
            timeout_duration,
            collector_handle
        ).await;

        match result {
            Ok(Ok(count)) => {
                info!("Collection completed, total samples: {}", count);
            }
            Ok(Err(e)) => {
                error!("Collection task error: {}", e);
            }
            Err(_) => {
                warn!("Collection timeout");
            }
        }

        // 停止分析器
        self.stop_profiler()?;

        // 生成分析结果
        let analysis_result = self.generate_result()?;
        self.session.set_result(analysis_result.clone());

        Ok(analysis_result)
    }

    /// 设置ETW控制器和事件回调
    fn setup_etw_controller(&mut self) -> Result<()> {
        // 创建ETW控制器
        let etw_controller = EtwController::new(&self.config)?;
        
        // 获取堆栈管理器和聚合器的引用，用于事件处理
        let stack_manager = Arc::new(Mutex::new(None::<StackManager>));
        let aggregator = Arc::clone(&self.aggregator);
        let target_process = self.config.target_process.clone();
        
        etw_controller.add_event_callback(move |event| {
            match event {
                ProcessedEvent::Sample(sample) => {
                    // 检查目标进程过滤
                    if let Some(ref target) = target_process {
                        if let Some(pid) = target.as_pid() {
                            if sample.process_id != pid {
                                return;
                            }
                        }
                    }
                    
                    trace!("Processing sample event: pid={}, tid={}, ip=0x{:x}",
                        sample.process_id, sample.thread_id, sample.instruction_pointer);
                    
                    // 将采样事件传递给堆栈管理器处理
                    if let Ok(manager_guard) = stack_manager.lock() {
                        if let Some(ref manager) = *manager_guard {
                            match manager.on_sample(sample) {
                                Ok(Some(stack)) => {
                                    // 堆栈收集成功，传递给聚合器
                                    if let Ok(mut agg) = aggregator.lock() {
                                        if let Err(e) = agg.process_stack(&stack) {
                                            warn!("Failed to process stack: {}", e);
                                        } else {
                                            trace!("Stack processed successfully, {} frames", stack.total_depth());
                                        }
                                    }
                                }
                                Ok(None) => {
                                    // 堆栈被过滤器排除
                                    trace!("Stack filtered out");
                                }
                                Err(e) => {
                                    // 堆栈收集失败
                                    trace!("Failed to collect stack: {}", e);
                                }
                            }
                        }
                    }
                }
                ProcessedEvent::ProcessStart(context) => {
                    debug!("Process started: PID={}, Name={}", context.pid, context.name);
                }
                ProcessedEvent::ProcessEnd(pid, _, exit_code) => {
                    debug!("Process ended: PID={}, ExitCode={}", pid, exit_code);
                }
                ProcessedEvent::ThreadStart(context) => {
                    trace!("Thread started: TID={}, PID={}", context.tid, context.pid);
                }
                ProcessedEvent::ThreadEnd(tid, _) => {
                    trace!("Thread ended: TID={}", tid);
                }
                ProcessedEvent::ImageLoad(pid, info) => {
                    trace!("Image loaded: PID={}, Name={}, Base=0x{:016X}",
                        pid, info.name, info.base_address);
                }
                _ => {}
            }
        });
        
        info!("ETW event callback registered");
        self.etw_controller = Some(etw_controller);
        
        Ok(())
    }

    /// 停止分析器（内部方法，避免借用冲突）
    fn stop_profiler(&mut self) -> Result<()> {
        info!("Stopping profiler");
        
        if !self.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        self.session.mark_stopping();
        self.running.store(false, Ordering::SeqCst);

        // 发送关闭信号
        if let Some(ref tx) = self.shutdown_tx {
            let _ = tx.try_send(());
        }

        // 停止ETW会话
        if let Some(ref mut session) = self.etw_session {
            let _ = session.stop();
        }

        // 停止ETW控制器
        if let Some(ref controller) = self.etw_controller {
            let _ = controller.shutdown_all();
        }

        // 关闭堆栈管理器
        let _ = self.stack_manager.shutdown();

        // 关闭符号管理器
        self.symbol_manager.shutdown()?;

        self.session.mark_completed();
        info!("Profiler stopped");

        Ok(())
    }

    /// 附加到已运行的进程
    ///
    /// # 参数
    /// - `pid`: 目标进程ID
    pub async fn attach_to_process(&mut self, pid: u32) -> Result<()> {
        info!("Attaching to process {}", pid);

        // 验证进程存在
        if !Self::process_exists(pid) {
            return Err(ProcessError::with_pid("Process not found", pid).into());
        }

        // 设置目标进程
        self.config.target_process = Some(crate::config::TargetProcess::Pid(pid));
        
        // 更新会话
        self.session.mark_running(Some(pid));

        // 创建符号解析器
        self.symbol_manager.create_resolver(pid)?;

        // 设置ETW进程过滤
        if let Some(ref controller) = self.etw_controller {
            controller.set_global_process_filter(&[pid])?;
        }

        info!("Successfully attached to process {}", pid);
        Ok(())
    }

    /// 启动并分析新进程
    ///
    /// # 参数
    /// - `path`: 可执行文件路径
    /// - `args`: 命令行参数
    pub async fn launch_process(&mut self, path: &Path, args: &[String]) -> Result<Child> {
        info!("Launching process: {:?} with args: {:?}", path, args);

        use std::process::Command;

        // 检查文件存在
        if !path.exists() {
            return Err(ProcessError::with_name("Executable not found", path.display().to_string()).into());
        }

        // 启动进程
        let mut cmd = Command::new(path);
        cmd.args(args);
        
        let child = cmd.spawn().map_err(|e| {
            ProcessError::with_name(format!("Failed to spawn process: {}", e), path.display().to_string())
        })?;

        let pid = child.id();
        info!("Launched process with PID: {}", pid);

        // 等待进程初始化
        tokio::time::sleep(Duration::from_millis(100)).await;

        // 附加到新进程
        self.attach_to_process(pid).await?;

        Ok(child)
    }

    /// 停止性能分析
    pub fn stop(&mut self) -> Result<()> {
        info!("Stopping profiler");
        
        if !self.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        self.session.mark_stopping();
        self.running.store(false, Ordering::SeqCst);

        // 发送关闭信号
        if let Some(ref tx) = self.shutdown_tx {
            let _ = tx.try_send(());
        }

        // 停止ETW会话
        if let Some(ref mut session) = self.etw_session {
            session.stop()?;
        }

        // 停止ETW控制器
        if let Some(ref controller) = self.etw_controller {
            controller.shutdown_all()?;
        }

        // 关闭堆栈管理器
        self.stack_manager.shutdown()?;

        // 关闭符号管理器
        self.symbol_manager.shutdown()?;

        self.session.mark_completed();
        info!("Profiler stopped");

        Ok(())
    }

    /// 获取当前进度
    pub fn get_progress(&self) -> ProfilerProgress {
        let stats = self.session.stats();
        
        ProfilerProgress {
            samples_collected: stats.sample_count,
            stacks_resolved: stats.resolved_stacks,
            threads_tracked: stats.thread_count,
            elapsed_time: stats.elapsed,
            status: stats.state.to_string(),
        }
    }

    /// 获取配置
    pub fn config(&self) -> &ProfilerConfig {
        &self.config
    }

    /// 获取会话
    pub fn session(&self) -> &ProfilerSession {
        &self.session
    }

    /// 检查是否正在运行
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// 生成分析报告
    pub fn generate_report(&self, output_path: &Path) -> Result<()> {
        info!("Generating report to: {:?}", output_path);

        let result = self.session.get_result()
            .ok_or_else(|| ProfilerError::Generic("No analysis result available".to_string()))?;

        let report_config = ReportConfig::default()
            .with_title("ETW Performance Profile Report".to_string())
            .with_system_functions(self.config.include_system_calls);

        let generator = CsvReportGenerator::new(report_config);
        generator.generate(&result, output_path)?;

        info!("Report generated successfully");
        Ok(())
    }


    /// 生成分析结果
    fn generate_result(&self) -> Result<AnalysisResult> {
        let process_stats = ProcessStats::new(
            self.session.target_process_id().unwrap_or(0)
        );

        let mut result = AnalysisResult::new(process_stats);
        
        // 从聚合器获取数据
        if let Ok(aggregator) = self.aggregator.lock() {
            result.total_samples = aggregator.total_samples();
            
            // 复制线程统计
            for (thread_id, thread_stats) in aggregator.get_all_thread_stats() {
                result.add_thread_stats(thread_stats.clone());
            }
            
            info!(
                "Generated result: {} samples, {} threads, {} functions",
                result.total_samples,
                result.thread_stats.len(),
                result.thread_stats.values().map(|t| t.function_stats.len()).sum::<usize>()
            );
        }

        result.analysis_start_time = self.session.start_time().elapsed().as_micros() as u64;
        result.analysis_end_time = result.analysis_start_time +
            self.session.elapsed().as_micros() as u64;

        Ok(result)
    }

    /// 检查进程是否存在
    fn process_exists(pid: u32) -> bool {
        #[cfg(windows)]
        {
            use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION};
            use windows::Win32::Foundation::CloseHandle;
            
            unsafe {
                if let Ok(handle) = OpenProcess(PROCESS_QUERY_INFORMATION, false, pid) {
                    let _ = CloseHandle(handle);
                    true
                } else {
                    false
                }
            }
        }
        #[cfg(not(windows))]
        {
            // 非Windows平台，假设存在
            true
        }
    }

    /// 处理采样事件
    fn handle_sample(&self, event: &SampleEvent) -> Result<Option<CollectedStack>> {
        // 检查目标进程过滤
        if let Some(ref target) = self.config.target_process {
            if let Some(pid) = target.as_pid() {
                if event.process_id != pid {
                    return Ok(None);
                }
            }
        }

        // 更新符号管理器（如果需要）
        // 这里可以处理模块加载事件等

        // 解析采样地址
        if let Ok(Some(_symbol)) = self.symbol_manager.resolve_sample(
            event.process_id, 
            event.instruction_pointer
        ) {
            // 符号解析成功，可以记录统计
        }

        // 返回None表示此函数需要完整实现堆栈收集
        Ok(None)
    }
}

impl Drop for Profiler {
    fn drop(&mut self) {
        if self.is_running() {
            let _ = self.stop();
        }
    }
}

/// 便捷函数：快速启动性能分析
///
/// # 参数
/// - `config`: 性能分析配置
///
/// # 返回
/// 性能分析器实例
pub fn create_profiler(config: ProfilerConfig) -> Result<Profiler> {
    Profiler::new(config)
}

/// 便捷函数：附加到进程
///
/// # 参数
/// - `pid`: 进程ID
/// - `output_path`: 输出文件路径
///
/// # 返回
/// 分析结果
pub async fn attach_and_profile(pid: u32, output_path: &Path) -> Result<AnalysisResult> {
    let config = ProfilerConfig::new()
        .with_pid(pid)
        .with_output_path(output_path);
    
    let mut profiler = Profiler::new(config)?;
    profiler.attach_to_process(pid).await?;
    profiler.run().await
}

/// 便捷函数：启动并分析新进程
///
/// # 参数
/// - `exe_path`: 可执行文件路径
/// - `args`: 命令行参数
/// - `output_path`: 输出文件路径
///
/// # 返回
/// 分析结果
pub async fn launch_and_profile(
    exe_path: &Path, 
    args: &[String], 
    output_path: &Path
) -> Result<AnalysisResult> {
    let config = ProfilerConfig::new()
        .with_output_path(output_path);
    
    let mut profiler = Profiler::new(config)?;
    let mut child = profiler.launch_process(exe_path, args).await?;
    
    let result = profiler.run().await;
    
    // 确保子进程被清理
    let _ = child.kill();
    
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_profiler_creation() {
        let config = ProfilerConfig::new()
            .with_pid(1234)
            .with_output_path("test.csv");
        
        let profiler = Profiler::new(config);
        assert!(profiler.is_ok());
        
        let p = profiler.unwrap();
        assert!(!p.is_running());
        assert_eq!(p.config().session_name, "EtwProfilerSession");
    }

    #[test]
    fn test_profiler_progress_default() {
        let progress = ProfilerProgress::default();
        assert_eq!(progress.samples_collected, 0);
        assert_eq!(progress.stacks_resolved, 0);
        assert_eq!(progress.threads_tracked, 0);
    }

    #[test]
    fn test_process_exists() {
        // 当前进程应该存在
        assert!(Profiler::process_exists(std::process::id()));
        // PID 0 通常不存在
        assert!(!Profiler::process_exists(0));
    }
}
