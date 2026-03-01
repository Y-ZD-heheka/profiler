//! 配置管理模块
//!
//! 提供配置结构定义、命令行参数解析和配置文件加载功能。
//! 支持从命令行参数或 TOML 配置文件加载配置。

use crate::error::{ConfigError, Result};
use clap::{Parser, ValueEnum};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 目标进程标识
///
/// 支持通过进程 ID 或进程路径/名称指定目标进程。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TargetProcess {
    /// 通过进程 ID 指定
    Pid(u32),
    /// 通过进程路径或名称指定
    Path(String),
}

impl TargetProcess {
    /// 创建 PID 类型的目标进程
    pub fn pid(pid: u32) -> Self {
        Self::Pid(pid)
    }

    /// 创建路径类型的目标进程
    pub fn path(path: impl Into<String>) -> Self {
        Self::Path(path.into())
    }

    /// 获取 PID（如果是 PID 类型）
    pub fn as_pid(&self) -> Option<u32> {
        match self {
            Self::Pid(pid) => Some(*pid),
            _ => None,
        }
    }

    /// 获取路径（如果是路径类型）
    pub fn as_path(&self) -> Option<&str> {
        match self {
            Self::Path(path) => Some(path),
            _ => None,
        }
    }
}

impl std::fmt::Display for TargetProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pid(pid) => write!(f, "PID:{}", pid),
            Self::Path(path) => write!(f, "{}", path),
        }
    }
}

impl From<u32> for TargetProcess {
    fn from(pid: u32) -> Self {
        Self::Pid(pid)
    }
}

impl From<String> for TargetProcess {
    fn from(path: String) -> Self {
        Self::Path(path)
    }
}

impl From<&str> for TargetProcess {
    fn from(path: &str) -> Self {
        Self::Path(path.to_string())
    }
}

/// 性能分析配置结构体
///
/// 包含性能分析会话的所有配置参数。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfilerConfig {
    /// 会话名称
    ///
    /// 用于标识 ETW 会话的名称，默认为 "EtwProfilerSession"。
    #[serde(default = "default_session_name")]
    pub session_name: String,

    /// 目标进程
    ///
    /// 要分析的进程，可以是 PID 或进程路径。
    pub target_process: Option<TargetProcess>,

    /// 输出 CSV 文件路径
    ///
    /// 分析结果输出的文件路径，默认为 "profile_output.csv"。
    #[serde(default = "default_output_path")]
    pub output_path: PathBuf,

    /// 采样间隔（毫秒）
    ///
    /// 两次采样之间的时间间隔，默认为 1ms。
    #[serde(default = "default_sample_interval")]
    pub sample_interval_ms: u32,

    /// 是否启用堆栈遍历
    ///
    /// 启用后会在每个采样点捕获调用堆栈，默认为 true。
    #[serde(default = "default_enable_stack_walk")]
    pub enable_stack_walk: bool,

    /// 最大堆栈深度
    ///
    /// 捕获调用堆栈的最大深度，默认为 32 帧。
    #[serde(default = "default_max_stack_depth")]
    pub max_stack_depth: u32,

    /// 额外的符号搜索路径
    ///
    /// PDB 符号文件的额外搜索路径列表。
    #[serde(default)]
    pub symbol_paths: Vec<PathBuf>,

    /// 会话持续时间（秒）
    ///
    /// 性能分析会话的运行时间，0 表示无限运行直到手动停止。
    #[serde(default)]
    pub duration_secs: u64,

    /// 是否包含系统调用
    ///
    /// 是否分析系统模块中的函数调用，默认为 false。
    #[serde(default)]
    pub include_system_calls: bool,

    /// 最小采样时间（微秒）
    ///
    /// 低于此时间的采样将被过滤，默认为 0（不过滤）。
    #[serde(default)]
    pub min_sample_time_us: u64,
}

impl Default for ProfilerConfig {
    fn default() -> Self {
        Self {
            session_name: default_session_name(),
            target_process: None,
            output_path: default_output_path(),
            sample_interval_ms: default_sample_interval(),
            enable_stack_walk: default_enable_stack_walk(),
            max_stack_depth: default_max_stack_depth(),
            symbol_paths: Vec::new(),
            duration_secs: 0,
            include_system_calls: false,
            min_sample_time_us: 0,
        }
    }
}

// 默认值函数
fn default_session_name() -> String {
    "EtwProfilerSession".to_string()
}

fn default_output_path() -> PathBuf {
    PathBuf::from("profile_output.csv")
}

fn default_sample_interval() -> u32 {
    1
}

fn default_enable_stack_walk() -> bool {
    true
}

fn default_max_stack_depth() -> u32 {
    32
}

impl ProfilerConfig {
    /// 创建默认配置
    pub fn new() -> Self {
        Self::default()
    }

    /// 从 TOML 配置文件加载配置
    ///
    /// # 参数
    /// - `path`: 配置文件路径
    ///
    /// # 错误
    /// 如果文件不存在或解析失败，返回 `ConfigError`。
    ///
    /// # 示例
    /// ```
    /// let config = ProfilerConfig::from_file("config.toml")?;
    /// ```
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path).map_err(|e| {
            ConfigError::with_file(
                format!("Failed to read config file: {}", e),
                path.display().to_string(),
                None,
            )
        })?;

        let config: ProfilerConfig = toml::from_str(&content).map_err(|e| {
            ConfigError::with_file(
                format!("Failed to parse config file: {}", e),
                path.display().to_string(),
                None,
            )
        })?;

        config.validate()?;
        Ok(config)
    }

    /// 将配置保存到 TOML 文件
    ///
    /// # 参数
    /// - `path`: 目标文件路径
    pub fn save_to_file(&self, path: impl AsRef<std::path::Path>) -> Result<()> {
        let path = path.as_ref();
        let content = toml::to_string_pretty(self).map_err(|e| {
            ConfigError::with_file(
                format!("Failed to serialize config: {}", e),
                path.display().to_string(),
                None,
            )
        })?;

        std::fs::write(path, content).map_err(|e| {
            ConfigError::with_file(
                format!("Failed to write config file: {}", e),
                path.display().to_string(),
                None,
            )
        })?;

        Ok(())
    }

    /// 验证配置有效性
    ///
    /// 检查配置参数是否在有效范围内。
    pub fn validate(&self) -> Result<()> {
        if self.session_name.is_empty() {
            return Err(ConfigError::with_field(
                "Session name cannot be empty",
                "session_name",
            )
            .into());
        }

        if self.sample_interval_ms == 0 {
            return Err(ConfigError::with_field(
                "Sample interval must be greater than 0",
                "sample_interval_ms",
            )
            .into());
        }

        if self.max_stack_depth == 0 {
            return Err(ConfigError::with_field(
                "Max stack depth must be greater than 0",
                "max_stack_depth",
            )
            .into());
        }

        if self.max_stack_depth > 1024 {
            return Err(ConfigError::with_field(
                "Max stack depth cannot exceed 1024",
                "max_stack_depth",
            )
            .into());
        }

        Ok(())
    }

    /// 设置目标进程（PID）
    pub fn with_pid(mut self, pid: u32) -> Self {
        self.target_process = Some(TargetProcess::Pid(pid));
        self
    }

    /// 设置目标进程（路径）
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.target_process = Some(TargetProcess::Path(path.into()));
        self
    }

    /// 设置输出路径
    pub fn with_output_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.output_path = path.into();
        self
    }

    /// 设置采样间隔
    pub fn with_sample_interval(mut self, interval_ms: u32) -> Self {
        self.sample_interval_ms = interval_ms;
        self
    }

    /// 设置是否启用堆栈遍历
    pub fn with_stack_walk(mut self, enable: bool) -> Self {
        self.enable_stack_walk = enable;
        self
    }

    /// 添加符号搜索路径
    pub fn add_symbol_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.symbol_paths.push(path.into());
        self
    }
}

/// 命令行参数定义
///
/// 使用 clap derive 宏定义的命令行参数结构。
#[derive(Parser, Debug, Clone)]
#[command(
    name = "etw-profiler",
    about = "A Windows performance profiler based on ETW",
    version
)]
pub struct CliArgs {
    /// 目标进程 PID
    #[arg(short, long, conflicts_with = "target_path")]
    pub pid: Option<u32>,

    /// 目标进程路径或名称
    #[arg(short, long, conflicts_with = "pid")]
    pub target_path: Option<String>,

    /// 配置文件路径
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// 输出 CSV 文件路径
    #[arg(short, long)]
    pub output: Option<PathBuf>,

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
    #[arg(short, long, default_value = "0")]
    pub duration: u64,

    /// 包含系统调用
    #[arg(long)]
    pub include_system: bool,

    /// 生成示例配置文件
    #[arg(long)]
    pub generate_config: Option<PathBuf>,

    /// 日志级别
    #[arg(long, default_value = "info")]
    pub log_level: LogLevel,
}

/// 日志级别枚举
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum LogLevel {
    /// 错误级别
    Error,
    /// 警告级别
    Warn,
    /// 信息级别
    Info,
    /// 调试级别
    Debug,
    /// 追踪级别
    Trace,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Error => write!(f, "error"),
            Self::Warn => write!(f, "warn"),
            Self::Info => write!(f, "info"),
            Self::Debug => write!(f, "debug"),
            Self::Trace => write!(f, "trace"),
        }
    }
}

impl From<CliArgs> for ProfilerConfig {
    /// 从命令行参数转换为配置
    ///
    /// 命令行参数优先级高于配置文件中的默认值。
    fn from(args: CliArgs) -> Self {
        let mut config = ProfilerConfig::new();

        // 设置目标进程
        if let Some(pid) = args.pid {
            config.target_process = Some(TargetProcess::Pid(pid));
        } else if let Some(path) = args.target_path {
            config.target_process = Some(TargetProcess::Path(path));
        }

        // 设置其他参数
        if let Some(output) = args.output {
            config.output_path = output;
        }

        config.sample_interval_ms = args.interval;
        config.enable_stack_walk = !args.no_stack_walk;
        config.max_stack_depth = args.max_depth;
        config.symbol_paths = args.symbol_path;
        config.duration_secs = args.duration;
        config.include_system_calls = args.include_system;

        config
    }
}

/// 加载配置的便捷函数
///
/// 优先从命令行参数指定的配置文件加载，否则使用命令行参数直接构建配置。
///
/// # 参数
/// - `args`: 命令行参数
///
/// # 返回
/// 合并后的配置对象
pub fn load_config(args: &CliArgs) -> Result<ProfilerConfig> {
    // 如果请求生成示例配置
    if let Some(path) = &args.generate_config {
        let example = ProfilerConfig::default();
        example.save_to_file(path)?;
        println!("Example config generated at: {}", path.display());
        std::process::exit(0);
    }

    // 如果有配置文件，从文件加载并合并命令行参数
    if let Some(config_path) = &args.config {
        let mut config = ProfilerConfig::from_file(config_path)?;

        // 命令行参数覆盖配置文件
        if args.pid.is_some() || args.target_path.is_some() {
            if let Some(pid) = args.pid {
                config.target_process = Some(TargetProcess::Pid(pid));
            } else if let Some(path) = &args.target_path {
                config.target_process = Some(TargetProcess::Path(path.clone()));
            }
        }

        if args.output.is_some() {
            config.output_path = args.output.clone().unwrap();
        }

        if args.interval != 1 {
            config.sample_interval_ms = args.interval;
        }

        if args.no_stack_walk {
            config.enable_stack_walk = false;
        }

        if args.max_depth != 32 {
            config.max_stack_depth = args.max_depth;
        }

        if !args.symbol_path.is_empty() {
            config.symbol_paths.extend(args.symbol_path.clone());
        }

        if args.duration != 0 {
            config.duration_secs = args.duration;
        }

        if args.include_system {
            config.include_system_calls = true;
        }

        config.validate()?;
        Ok(config)
    } else {
        // 从命令行参数构建配置
        let config = ProfilerConfig::from(args.clone());
        config.validate()?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ProfilerConfig::default();
        assert_eq!(config.session_name, "EtwProfilerSession");
        assert_eq!(config.sample_interval_ms, 1);
        assert!(config.enable_stack_walk);
        assert_eq!(config.max_stack_depth, 32);
    }

    #[test]
    fn test_config_builder_pattern() {
        let config = ProfilerConfig::new()
            .with_pid(1234)
            .with_output_path("output.csv")
            .with_sample_interval(10)
            .with_stack_walk(false);

        assert_eq!(config.target_process, Some(TargetProcess::Pid(1234)));
        assert_eq!(config.output_path, PathBuf::from("output.csv"));
        assert_eq!(config.sample_interval_ms, 10);
        assert!(!config.enable_stack_walk);
    }

    #[test]
    fn test_config_validation() {
        let config = ProfilerConfig {
            sample_interval_ms: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());

        let config = ProfilerConfig {
            max_stack_depth: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());

        let config = ProfilerConfig {
            max_stack_depth: 2048,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_target_process_display() {
        let pid_target = TargetProcess::Pid(1234);
        assert_eq!(pid_target.to_string(), "PID:1234");

        let path_target = TargetProcess::Path("test.exe".to_string());
        assert_eq!(path_target.to_string(), "test.exe");
    }

    #[test]
    fn test_cli_args_to_config() {
        let args = CliArgs {
            pid: Some(1234),
            target_path: None,
            config: None,
            output: Some(PathBuf::from("test.csv")),
            interval: 5,
            no_stack_walk: true,
            max_depth: 64,
            symbol_path: vec![PathBuf::from("symbols")],
            duration: 60,
            include_system: true,
            generate_config: None,
            log_level: LogLevel::Info,
        };

        let config: ProfilerConfig = args.into();
        assert_eq!(config.target_process, Some(TargetProcess::Pid(1234)));
        assert_eq!(config.output_path, PathBuf::from("test.csv"));
        assert_eq!(config.sample_interval_ms, 5);
        assert!(!config.enable_stack_walk);
        assert_eq!(config.max_stack_depth, 64);
        assert_eq!(config.duration_secs, 60);
        assert!(config.include_system_calls);
    }

    #[test]
    fn test_config_serialization() {
        let config = ProfilerConfig {
            target_process: Some(TargetProcess::Pid(1234)),
            output_path: PathBuf::from("test.csv"),
            symbol_paths: vec![PathBuf::from("symbols")],
            ..Default::default()
        };

        let toml_str = toml::to_string(&config).unwrap();
        assert!(toml_str.contains("session_name"));
        assert!(toml_str.contains("EtwProfilerSession"));

        let parsed: ProfilerConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.session_name, config.session_name);
    }
}
