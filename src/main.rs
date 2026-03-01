//! ETW Performance Profiler
//!
//! A Windows performance profiler based on Event Tracing for Windows (ETW).
//! Provides CPU sampling profiling with call stack capture and symbol resolution.

#![warn(missing_docs)]
#![warn(rust_2018_idioms)]

// ============================================================================
// 模块声明
// ============================================================================

/// 错误类型定义模块
///
/// 提供统一的错误类型 `ProfilerError` 和相关的错误处理工具。
pub mod error;

/// 配置管理模块
///
/// 提供配置结构定义、命令行参数解析和配置文件加载功能。
pub mod config;

/// 核心数据结构模块
///
/// 定义采样事件、堆栈帧、调用堆栈、统计信息等核心数据结构。
pub mod types;

/// ETW 事件采集模块
///
/// 提供基于 Windows ETW API 的高性能事件采集功能，
/// 支持 CPU 采样、进程/线程生命周期跟踪和模块加载事件捕获。
pub mod etw;

/// 符号解析模块
///
/// 提供 PDB 符号文件的加载、缓存和解析功能。
/// 使用 Windows DbgHelp API 实现符号解析，支持多进程符号管理。
pub mod symbols;

/// 堆栈遍历模块
///
/// 提供调用堆栈的展开、解析和收集功能，将 ETW 采样事件中的原始地址
/// 转换为带符号信息的完整调用堆栈。
pub mod stackwalker;

/// 数据分析与统计模块
///
/// 提供性能分析数据的处理、统计和可视化功能。
/// 包括堆栈数据统计、线程分析、火焰图生成和结果导出等功能。
pub mod analysis;

/// 性能报告生成模块
///
/// 提供专业的性能分析报告生成功能，支持CSV格式导出。
/// 包括按线程分类的函数统计、热点路径分析、调用堆栈详情等多种报告类型。
pub mod report;

/// 性能分析器协调器模块
///
/// 协调所有模块的工作，提供统一的性能分析接口。
pub mod profiler;

/// 会话状态管理模块
///
/// 提供性能分析会话的状态管理和生命周期控制。
pub mod session;

/// CLI 模块
///
/// 提供命令行界面相关的类型和功能。
pub mod cli;

// ============================================================================
// 公共导出
// ============================================================================

pub use etw::{
    EtwController, EtwSession, EventProcessor, EventHandler,
    KernelProviderFlags, ProviderConfig, ProcessedEvent, SessionMode,
    generate_unique_session_name, is_valid_session_name,
};

pub use config::{load_config, CliArgs, ProfilerConfig, TargetProcess};
pub use error::{ConfigError, EtwError, ProcessError, ProfilerError, Result, SymbolError};
pub use types::{
    Address, CallStack, FunctionStats, ModuleInfo, ProcessId, ProcessStats, SampleEvent,
    StackFrame, SymbolInfo, ThreadId, ThreadStats, Timestamp,
};
pub use symbols::{
    DbgHelpResolver, SymbolCache, SymbolManager, SymbolResolver, PdbLocator,
    SymbolManagerStats, CacheStats, SharedSymbolCache, build_symbol_search_path,
};
pub use profiler::{Profiler, ProfilerProgress, create_profiler, attach_and_profile, launch_and_profile};
pub use session::{ProfilerSession, SessionState, SessionManager, SessionStats};

// ============================================================================
// 应用入口
// ============================================================================

use clap::Parser;
use cli::{Cli, Commands, ProgressReporter};
use console::style;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tracing::{error, info, warn};

/// 应用程序退出码
mod exit_code {
    /// 成功
    pub const SUCCESS: i32 = 0;
    /// 一般错误
    pub const ERROR: i32 = 1;
    /// 用户中断
    pub const INTERRUPTED: i32 = 130;
    /// 配置错误
    pub const CONFIG_ERROR: i32 = 2;
}

/// 异步主函数
#[tokio::main]
async fn main() {
    // 初始化日志系统
    init_logging();

    info!("Starting ETW Profiler v{}", env!("CARGO_PKG_VERSION"));

    // 解析命令行参数
    let cli = Cli::parse();

    // 设置日志级别
    set_log_level(&cli);

    // 处理子命令
    let exit_code = match run(cli).await {
        Ok(_) => exit_code::SUCCESS,
        Err(e) => {
            cli::output::print_error(&e);
            if matches!(e, ProfilerError::IoError(_) | ProfilerError::Generic(_)) {
                exit_code::ERROR
            } else {
                exit_code::ERROR
            }
        }
    };

    std::process::exit(exit_code);
}

/// 主运行逻辑
async fn run(cli: Cli) -> Result<()> {
    // 处理生成配置文件的请求
    if let Some(ref path) = cli.generate_config {
        return generate_example_config(path);
    }

    // 根据子命令执行相应操作
    match &cli.command {
        Some(Commands::Launch { path, args, .. }) => {
            run_launch_mode(&cli, path, args).await
        }
        Some(Commands::Attach { pid, .. }) => {
            run_attach_mode(&cli, *pid).await
        }
        Some(Commands::Analyze { file, .. }) => {
            run_analyze_mode(&cli, file).await
        }
        Some(Commands::ListProcesses { user_only }) => {
            run_list_processes(*user_only).await
        }
        None => {
            // 使用直接参数模式
            if let Some(pid) = cli.pid {
                run_attach_mode(&cli, pid).await
            } else if let Some(ref path) = cli.target_path {
                run_launch_mode(&cli, Path::new(path), &[]).await
            } else {
                // 显示欢迎信息和帮助
                cli::output::print_welcome();
                cli::output::print_help_hint();
                Ok(())
            }
        }
    }
}

/// 启动模式：启动并分析新进程
async fn run_launch_mode(cli: &Cli, path: &Path, args: &[String]) -> Result<()> {
    cli::output::print_welcome();
    
    println!("Launching process: {:?}", path);
    if !args.is_empty() {
        println!("Arguments: {:?}", args);
    }

    // 创建配置
    let config = create_config_from_cli(cli)?;
    let output_path = cli.get_output_path();

    // 创建分析器
    let mut profiler = Profiler::new(config)?;

    // 启动目标进程
    let mut child = profiler.launch_process(path, args).await?;
    
    println!("Process started with PID: {}", child.id());
    cli::output::print_interrupt_hint();

    // 运行分析
    run_profiler_with_ui(cli, &mut profiler, &output_path).await?;

    // 清理子进程
    let _ = child.kill();
    let _ = child.wait();

    Ok(())
}

/// 附加模式：附加到已运行的进程
async fn run_attach_mode(cli: &Cli, pid: u32) -> Result<()> {
    cli::output::print_welcome();
    
    println!("Attaching to process {}...", style(pid).cyan());

    // 创建配置
    let mut config = create_config_from_cli(cli)?;
    config.target_process = Some(config::TargetProcess::Pid(pid));
    let output_path = cli.get_output_path();

    // 创建分析器
    let mut profiler = Profiler::new(config)?;

    // 附加到进程
    profiler.attach_to_process(pid).await?;
    
    println!("Successfully attached to process {}", style(pid).green());
    cli::output::print_interrupt_hint();

    // 运行分析
    run_profiler_with_ui(cli, &mut profiler, &output_path).await?;

    Ok(())
}

/// 分析模式：分析现有的ETW日志文件
async fn run_analyze_mode(_cli: &Cli, file: &Path) -> Result<()> {
    cli::output::print_welcome();
    
    println!("Analyzing ETW log file: {:?}", file);
    
    // TODO: 实现ETL文件分析
    // 目前返回一个占位结果
    println!("{}", style("ETL file analysis is not yet implemented").yellow());
    
    Ok(())
}

/// 列出可分析的进程
async fn run_list_processes(_user_only: bool) -> Result<()> {
    println!("{}", style("Listing running processes...").cyan());
    
    // 使用更简单的实现方式 - 通过执行系统命令
    #[cfg(windows)]
    {
        let output = std::process::Command::new("tasklist")
            .args(&["/FO", "CSV", "/NH"])
            .output()
            .map_err(|e| ProfilerError::IoError(e))?;
        
        let text = String::from_utf8_lossy(&output.stdout);
        let mut processes = Vec::new();
        
        for line in text.lines().take(50) {
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() >= 2 {
                let name = parts[0].trim_matches('"').to_string();
                if let Ok(pid) = parts[1].trim_matches('"').parse::<u32>() {
                    processes.push(cli::commands::ProcessInfo {
                        pid,
                        name,
                        exe_path: None,
                    });
                }
            }
        }
        
        cli::output::print_process_list(&processes);
    }
    
    #[cfg(not(windows))]
    {
        println!("{}", style("Process listing is only available on Windows").yellow());
    }
    
    Ok(())
}

/// 运行分析器并显示UI
async fn run_profiler_with_ui(
    cli: &Cli,
    profiler: &mut Profiler,
    output_path: &Path,
) -> Result<()> {
    // 创建进度报告器
    let mut reporter = if cli.is_non_interactive() {
        ProgressReporter::non_interactive()
    } else {
        let mut r = ProgressReporter::new();
        if cli.get_duration() > 0 {
            r.init_with_duration(Duration::from_secs(cli.get_duration()));
        } else {
            r.init_spinner("Profiling...");
        }
        r
    };

    // 设置Ctrl+C处理器
    let running = Arc::new(AtomicBool::new(true));
    let r = Arc::clone(&running);
    
    let session = profiler.session().clone();
    
    ctrlc::set_handler(move || {
        println!("\n{}", style("Interrupted by user").yellow());
        r.store(false, Ordering::SeqCst);
        session.cancel();
    }).map_err(|e| ProfilerError::Generic(format!("Failed to set Ctrl+C handler: {}", e)))?;

    // 启动分析任务
    let duration = if cli.get_duration() > 0 {
        Duration::from_secs(cli.get_duration())
    } else {
        Duration::from_secs(10) // 默认10秒
    };

    // 运行分析（带超时）
    let result = tokio::time::timeout(
        duration + Duration::from_secs(5),
        profiler.run()
    ).await;

    match result {
        Ok(Ok(analysis_result)) => {
            reporter.finish_with_success("Profiling completed successfully");
            cli::output::print_summary(&analysis_result);
            
            // 生成报告
            println!("Generating report to: {:?}", output_path);
            profiler.generate_report(output_path)?;
            cli::output::print_success(format!("Report saved to: {:?}", output_path));
            
            Ok(())
        }
        Ok(Err(e)) => {
            reporter.finish_with_error(format!("Profiling failed: {}", e));
            Err(e)
        }
        Err(_) => {
            // 超时，停止分析
            warn!("Profiling timeout reached");
            let _ = profiler.stop();
            
            // 尝试获取结果
            if let Some(result) = profiler.session().get_result() {
                reporter.finish_with_success("Profiling completed (timeout)");
                cli::output::print_summary(&result);
                
                // 生成报告
                println!("Generating report to: {:?}", output_path);
                profiler.generate_report(output_path)?;
                cli::output::print_success(format!("Report saved to: {:?}", output_path));
                
                Ok(())
            } else {
                reporter.finish_with_error("Profiling failed: no results");
                Err(ProfilerError::Generic("No analysis results available".to_string()))
            }
        }
    }
}

/// 从CLI参数创建配置
fn create_config_from_cli(cli: &Cli) -> Result<ProfilerConfig> {
    let mut config = if let Some(ref config_path) = cli.config {
        // 从文件加载配置
        ProfilerConfig::from_file(config_path)?
    } else {
        ProfilerConfig::default()
    };

    // 应用CLI参数覆盖
    config.output_path = cli.get_output_path();
    config.sample_interval_ms = cli.get_interval();
    config.enable_stack_walk = !cli.no_stack_walk;
    config.max_stack_depth = cli.max_depth;
    config.duration_secs = cli.get_duration();
    config.include_system_calls = cli.include_system;

    // 添加符号路径
    for path in &cli.symbol_path {
        config.symbol_paths.push(path.clone());
    }

    config.validate()?;
    Ok(config)
}

/// 生成示例配置文件
fn generate_example_config(path: &Path) -> Result<()> {
    let example = ProfilerConfig::default();
    example.save_to_file(path)?;
    
    cli::output::print_success(format!("Example configuration saved to: {:?}", path));
    println!();
    println!("You can edit this file and use it with: --config {:?}", path);
    
    Ok(())
}

/// 初始化日志系统
///
/// 配置 tracing-subscriber 使用环境过滤器，默认级别为 INFO。
fn init_logging() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_thread_ids(false)
        .with_line_number(false)
        .with_ansi(false)
        .init();
}

/// 根据命令行参数设置日志级别
fn set_log_level(cli: &Cli) {
    use tracing::Level;
    
    let level = match cli.log_level.as_str() {
        "error" => Level::ERROR,
        "warn" => Level::WARN,
        "info" => Level::INFO,
        "debug" => Level::DEBUG,
        "trace" => Level::TRACE,
        _ => Level::INFO,
    };

    info!("Log level set to: {:?}", level);
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        // 验证所有公共类型都可以访问
        let _: error::ProfilerError = error::EtwError::new("test").into();
        let _: error::ProfilerError = error::SymbolError::new("test").into();
        let _: error::ProfilerError = error::ConfigError::new("test").into();
        let _: error::ProfilerError = error::ProcessError::new("test").into();
    }

    #[test]
    fn test_config_integration() {
        use config::ProfilerConfig;
        use std::path::PathBuf;

        let config = ProfilerConfig::new()
            .with_pid(1234)
            .with_output_path("test.csv")
            .with_sample_interval(10);

        assert_eq!(config.sample_interval_ms, 10);
        assert_eq!(config.output_path, PathBuf::from("test.csv"));
    }

    #[test]
    fn test_types_integration() {
        use types::{CallStack, SampleEvent, StackFrame, ThreadStats};

        let event = SampleEvent::new(1000, 1234, 5678, 0x00400000);
        assert_eq!(event.process_id, 1234);

        let frame = StackFrame::new(0x00400000);
        assert_eq!(frame.address, 0x00400000);

        let mut stack = CallStack::new();
        stack.push(frame);
        assert_eq!(stack.len(), 1);

        let thread_stats = ThreadStats::new(5678, 1234);
        assert_eq!(thread_stats.thread_id, 5678);
    }
}
