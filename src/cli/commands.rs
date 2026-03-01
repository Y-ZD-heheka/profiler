//! CLI 子命令定义模块
//!
//! 定义性能分析工具的所有子命令和命令行参数。

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// 进程信息结构
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    /// 进程 ID
    pub pid: u32,
    /// 进程名称
    pub name: String,
    /// 可执行文件路径
    pub exe_path: Option<PathBuf>,
}

/// ETW 性能分析器 CLI
#[derive(Parser, Debug)]
#[command(
    name = "etw-profiler",
    about = "A Windows performance profiler based on ETW",
    version,
    author
)]
pub struct Cli {
    /// 子命令
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// 目标进程 PID（直接指定，无需子命令）
    #[arg(short, long, conflicts_with = "target_path")]
    pub pid: Option<u32>,

    /// 目标进程路径或名称（直接指定，无需子命令）
    #[arg(short, long, conflicts_with = "pid")]
    pub target_path: Option<String>,

    /// 输出 CSV 文件路径
    #[arg(short, long, default_value = "profile_output.csv")]
    pub output: PathBuf,

    /// 采样间隔（毫秒）
    #[arg(long, default_value = "1")]
    pub interval: u32,

    /// 禁用堆栈遍历
    #[arg(long)]
    pub no_stack_walk: bool,

    /// 最大堆栈深度
    #[arg(long, default_value = "32")]
    pub max_depth: u32,

    /// 额外的符号搜索路径（可多次指定）
    #[arg(long)]
    pub symbol_path: Vec<PathBuf>,

    /// 会话持续时间（秒），0 表示无限
    #[arg(short, long)]
    pub duration: Option<u64>,

    /// 包含系统调用
    #[arg(long)]
    pub include_system: bool,

    /// 配置文件路径
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// 生成示例配置文件
    #[arg(long)]
    pub generate_config: Option<PathBuf>,

    /// 日志级别
    #[arg(long, default_value = "info")]
    pub log_level: String,

    /// 非交互模式（无进度条）
    #[arg(long)]
    pub no_progress: bool,
}

/// CLI 子命令枚举
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// 启动并分析新进程
    Launch {
        /// 可执行文件路径
        path: PathBuf,
        /// 传递给目标程序的参数
        args: Vec<String>,
        /// 输出 CSV 文件路径
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// 采样间隔（毫秒）
        #[arg(long)]
        interval: Option<u32>,
        /// 会话持续时间（秒）
        #[arg(short, long)]
        duration: Option<u64>,
    },
    /// 附加到已运行的进程
    Attach {
        /// 进程 ID
        pid: u32,
        /// 输出 CSV 文件路径
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// 采样间隔（毫秒）
        #[arg(long)]
        interval: Option<u32>,
        /// 会话持续时间（秒）
        #[arg(short, long)]
        duration: Option<u64>,
    },
    /// 分析ETW日志文件
    Analyze {
        /// ETL 文件路径
        file: PathBuf,
        /// 输出 CSV 文件路径
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// 列出可分析的进程
    ListProcesses {
        /// 只显示用户进程
        #[arg(long)]
        user_only: bool,
    },
}

impl Cli {
    /// 获取输出路径（优先使用子命令中指定的）
    pub fn get_output_path(&self) -> PathBuf {
        match &self.command {
            Some(Commands::Launch { output, .. }) => output.clone().unwrap_or(self.output.clone()),
            Some(Commands::Attach { output, .. }) => output.clone().unwrap_or(self.output.clone()),
            Some(Commands::Analyze { output, .. }) => output.clone().unwrap_or(self.output.clone()),
            _ => self.output.clone(),
        }
    }

    /// 获取采样间隔（优先使用子命令中指定的）
    pub fn get_interval(&self) -> u32 {
        match &self.command {
            Some(Commands::Launch { interval, .. }) => interval.unwrap_or(self.interval),
            Some(Commands::Attach { interval, .. }) => interval.unwrap_or(self.interval),
            _ => self.interval,
        }
    }

    /// 获取持续时间（优先使用子命令中指定的）
    pub fn get_duration(&self) -> Option<u64> {
        match &self.command {
            Some(Commands::Launch { duration, .. }) => duration.or(self.duration),
            Some(Commands::Attach { duration, .. }) => duration.or(self.duration),
            _ => self.duration,
        }
    }

    /// 检查是否为非交互模式
    pub fn is_non_interactive(&self) -> bool {
        self.no_progress
    }
}
