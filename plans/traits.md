# 核心Trait和数据结构定义

本文档包含ETW性能分析工具的核心数据结构定义和所有trait接口。

## 基础类型定义

```rust
/// 会话句柄（唯一标识）
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SessionHandle(pub u64);

/// 事件处理器ID
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HandlerId(pub u64);

/// GUID类型（用于ETW Provider ID）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Guid(pub [u8; 16]);
```

## 核心数据结构

### 采样相关

```rust
/// 单个采样点，记录某一时刻的调用栈信息
#[derive(Debug, Clone)]
pub struct Sample {
    /// 采样时间戳（高精度计数器）
    pub timestamp: u64,
    /// 目标进程ID
    pub process_id: u32,
    /// 目标线程ID
    pub thread_id: u32,
    /// CPU核心编号
    pub cpu_core: u16,
    /// 调用栈信息
    pub stack_trace: StackTrace,
    /// 线程执行状态
    pub thread_state: ThreadState,
}

/// 调用栈信息
#[derive(Debug, Clone, Default)]
pub struct StackTrace {
    /// 栈帧列表，从顶部（最近调用）到底部
    pub frames: Vec<StackFrame>,
}

impl StackTrace {
    pub fn depth(&self) -> usize {
        self.frames.len()
    }
}

/// 单个栈帧信息
#[derive(Debug, Clone)]
pub struct StackFrame {
    /// 指令指针（地址）
    pub instruction_pointer: u64,
    /// 解析后的符号信息
    pub symbol: Option<SymbolInfo>,
    /// 模块（DLL/EXE）信息
    pub module: Option<ModuleInfo>,
}
```

### 线程状态枚举

```rust
/// 线程状态枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreadState {
    Running,
    Ready,
    Waiting { reason: WaitReason },
    Suspended,
    Terminated,
}

/// 等待原因
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WaitReason {
    UserRequest,
    IoCompletion,
    Kernel,
    Executive,
    FreePage,
    PageIn,
    PoolAllocation,
    DelayExecution,
    Suspended,
    UserRequest2,
    EventPairHigh,
    EventPairLow,
    LpcReceive,
    LpcReply,
    VirtualMemory,
    PageOut,
    Unknown,
}
```

### 符号与模块信息

```rust
/// 符号信息
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolInfo {
    /// 函数名称
    pub name: String,
    /// 函数地址
    pub address: u64,
    /// 函数大小
    pub size: u64,
    /// 源文件路径
    pub source_file: Option<PathBuf>,
    /// 源文件行号
    pub line_number: Option<u32>,
    /// 所属模块
    pub module_name: String,
}

/// 模块信息
#[derive(Debug, Clone)]
pub struct ModuleInfo {
    /// 模块名称
    pub name: String,
    /// 基地址
    pub base_address: u64,
    /// 模块大小
    pub size: u64,
    /// 模块路径
    pub path: PathBuf,
    /// PDB文件路径
    pub pdb_path: Option<PathBuf>,
    /// 是否已加载符号
    pub symbols_loaded: bool,
}
```

### 统计信息

```rust
/// 线程统计汇总
#[derive(Debug, Clone)]
pub struct ThreadStatistics {
    /// 线程ID
    pub thread_id: u32,
    /// 线程名称
    pub name: Option<String>,
    /// 总采样次数
    pub total_samples: u64,
    /// 各状态采样计数
    pub state_counts: HashMap<ThreadState, u64>,
    /// CPU使用时间（纳秒）
    pub cpu_time_ns: u64,
    /// 函数耗时统计
    pub function_stats: Vec<FunctionStats>,
}

/// 单个函数统计
#[derive(Debug, Clone)]
pub struct FunctionStats {
    /// 函数符号信息
    pub symbol: SymbolInfo,
    /// 采样命中次数
    pub hit_count: u64,
    /// 总自耗时（仅当前函数执行时间）
    pub self_time_ns: u64,
    /// 总包含耗时（包含子函数调用）
    pub inclusive_time_ns: u64,
    /// 平均调用深度
    pub avg_depth: f64,
}
```

### 配置与会话

```rust
/// 分析会话配置
#[derive(Debug, Clone)]
pub struct ProfilingConfig {
    /// 目标进程路径（Launch模式）
    pub target_path: Option<PathBuf>,
    /// 目标进程参数
    pub target_args: Vec<String>,
    /// 目标进程ID（Attach模式）
    pub target_pid: Option<u32>,
    /// 采样间隔（毫秒）
    pub sampling_interval_ms: u32,
    /// PDB搜索路径
    pub symbol_paths: Vec<PathBuf>,
    /// 符号缓存目录
    pub symbol_cache_dir: PathBuf,
    /// 输出文件路径
    pub output_path: PathBuf,
    /// 最大采样数限制
    pub max_samples: Option<u64>,
    /// 会话持续时间限制（秒）
    pub duration_limit_secs: Option<u64>,
    /// 是否排除系统线程
    pub exclude_system_threads: bool,
}

impl Default for ProfilingConfig {
    fn default() -> Self {
        Self {
            target_path: None,
            target_args: Vec::new(),
            target_pid: None,
            sampling_interval_ms: 10,
            symbol_paths: Vec::new(),
            symbol_cache_dir: PathBuf::from("C:\\SymbolCache"),
            output_path: PathBuf::from("profile_report.csv"),
            max_samples: None,
            duration_limit_secs: None,
            exclude_system_threads: true,
        }
    }
}

/// 分析会话状态
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Initializing,
    Running,
    Paused,
    Stopping,
    Completed,
    Error(String),
}
```

### ETW事件

```rust
/// ETW原始事件包装
#[derive(Debug, Clone)]
pub struct EtwEvent {
    /// 事件类型
    pub event_type: EtwEventType,
    /// 原始事件数据
    pub raw_data: Vec<u8>,
    /// 时间戳
    pub timestamp: u64,
    /// 进程ID
    pub process_id: u32,
    /// 线程ID
    pub thread_id: u32,
}

/// ETW事件类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EtwEventType {
    SampledProfile,
    ThreadStart,
    ThreadStop,
    ProcessStart,
    ProcessStop,
    ImageLoad,
    ImageUnload,
    StackWalk,
    ContextSwitch,
    Dpc,
    Interrupt,
    SystemCallEnter,
    SystemCallExit,
    Unknown(u16),
}

/// ETW配置
#[derive(Debug, Clone)]
pub struct EtwConfig {
    pub buffer_size_kb: u32,
    pub min_buffers: u32,
    pub max_buffers: u32,
    pub flush_timeout_secs: u32,
    pub enable_stack_walk: bool,
}

impl Default for EtwConfig {
    fn default() -> Self {
        Self {
            buffer_size_kb: 64,
            min_buffers: 2,
            max_buffers: 32,
            flush_timeout_secs: 1,
            enable_stack_walk: true,
        }
    }
}
```

### 其他辅助类型

```rust
/// 监控目标
#[derive(Debug, Clone)]
pub enum ProcessTarget {
    /// 启动新进程
    Launch { path: PathBuf, args: Vec<String> },
    /// 附加到现有进程
    Attach { pid: u32 },
}

/// 分析会话
#[derive(Debug, Clone)]
pub struct ProfilingSession {
    pub handle: SessionHandle,
    pub process_id: u32,
    pub start_time: Instant,
}

/// 分析结果数据
#[derive(Debug, Clone)]
pub struct ProfilingData {
    pub session_info: ProfilingSession,
    pub samples: Vec<Sample>,
    pub thread_stats: Vec<ThreadStatistics>,
    pub modules: Vec<ModuleInfo>,
    pub duration_ms: u64,
}

/// 报告数据结构
#[derive(Debug, Clone)]
pub struct Report {
    pub title: String,
    pub generated_at: String,
    pub data: ProfilingData,
    pub metadata: HashMap<String, String>,
}
```

## 错误类型定义

```rust
/// 统一错误类型
#[derive(Debug)]
pub enum ProfilerError {
    Session(SessionError),
    Symbol(SymbolError),
    Etw(EtwError),
    Event(EventError),
    Collector(CollectorError),
    Monitor(MonitorError),
    Launch(LaunchError),
    Report(ReportError),
    Format(FormatError),
    Config(String),
    Io(std::io::Error),
}

#[derive(Debug)]
pub struct SessionError(pub String);

#[derive(Debug)]
pub struct SymbolError(pub String);

#[derive(Debug)]
pub struct EtwError(pub String);

#[derive(Debug)]
pub struct EventError(pub String);

#[derive(Debug)]
pub struct CollectorError(pub String);

#[derive(Debug)]
pub struct MonitorError(pub String);

#[derive(Debug)]
pub struct LaunchError(pub String);

#[derive(Debug)]
pub struct ReportError(pub String);

#[derive(Debug)]
pub struct FormatError(pub String);

impl std::fmt::Display for ProfilerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfilerError::Session(e) => write!(f, "Session error: {}", e.0),
            ProfilerError::Symbol(e) => write!(f, "Symbol error: {}", e.0),
            ProfilerError::Etw(e) => write!(f, "ETW error: {}", e.0),
            ProfilerError::Event(e) => write!(f, "Event error: {}", e.0),
            ProfilerError::Collector(e) => write!(f, "Collector error: {}", e.0),
            ProfilerError::Monitor(e) => write!(f, "Monitor error: {}", e.0),
            ProfilerError::Launch(e) => write!(f, "Launch error: {}", e.0),
            ProfilerError::Report(e) => write!(f, "Report error: {}", e.0),
            ProfilerError::Format(e) => write!(f, "Format error: {}", e.0),
            ProfilerError::Config(s) => write!(f, "Config error: {}", s),
            ProfilerError::Io(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl std::error::Error for ProfilerError {}
```

## 核心Trait接口

### SessionManager - 会话管理器

```rust
/// 分析会话管理器
/// 
/// 负责管理分析会话的生命周期，包括创建、启动、暂停、恢复和停止。
/// 实现者需要确保线程安全（Send + Sync）。
pub trait SessionManager: Send + Sync {
    /// 创建新的分析会话
    fn create_session(&mut self, config: ProfilingConfig) -> Result<SessionHandle, SessionError>;
    
    /// 启动会话
    fn start_session(&mut self, handle: SessionHandle) -> Result<(), SessionError>;
    
    /// 暂停会话
    fn pause_session(&mut self, handle: SessionHandle) -> Result<(), SessionError>;
    
    /// 恢复会话
    fn resume_session(&mut self, handle: SessionHandle) -> Result<(), SessionError>;
    
    /// 停止会话
    fn stop_session(&mut self, handle: SessionHandle) -> Result<ProfilingData, SessionError>;
    
    /// 获取会话状态
    fn get_session_state(&self, handle: SessionHandle) -> SessionState;
    
    /// 获取当前配置
    fn get_config(&self, handle: SessionHandle) -> Option<&ProfilingConfig>;
    
    /// 列出所有活跃会话
    fn list_sessions(&self) -> Vec<SessionHandle>;
}
```

### EventHandler - 事件处理器

```rust
/// 事件处理器
/// 
/// 用于处理ETW事件的回调接口。可以实现多个处理器形成处理链。
pub trait EventHandler: Send + Sync {
    /// 处理ETW事件
    fn on_event(&mut self, event: &EtwEvent) -> Result<(), EventError>;
    
    /// 处理采样事件
    fn on_sample(&mut self, sample: &Sample) -> Result<(), EventError>;
    
    /// 处理线程创建事件
    fn on_thread_start(&mut self, thread_id: u32, process_id: u32) -> Result<(), EventError>;
    
    /// 处理线程终止事件
    fn on_thread_stop(&mut self, thread_id: u32) -> Result<(), EventError>;
    
    /// 处理模块加载事件
    fn on_module_load(&mut self, module: &ModuleInfo) -> Result<(), EventError>;
    
    /// 处理模块卸载事件
    fn on_module_unload(&mut self, base_address: u64) -> Result<(), EventError>;
}
```

### EventHandlerRegistry - 事件处理器注册表

```rust
/// 事件处理器注册表
/// 
/// 管理多个事件处理器的注册、注销和事件分发。
pub trait EventHandlerRegistry: Send + Sync {
    /// 注册处理器，返回处理器ID
    fn register(&mut self, handler: Box<dyn EventHandler>) -> HandlerId;
    
    /// 注销处理器，返回被移除的处理器
    fn unregister(&mut self, id: HandlerId) -> Option<Box<dyn EventHandler>>;
    
    /// 广播事件到所有处理器
    fn broadcast(&mut self, event: &EtwEvent) -> Result<(), EventError>;
    
    /// 获取已注册处理器数量
    fn handler_count(&self) -> usize;
    
    /// 清空所有处理器
    fn clear(&mut self);
}
```

### SymbolResolver - 符号解析器

```rust
/// 符号解析器接口
/// 
/// 负责解析内存地址到符号名称、源码位置等信息。
/// 支持PDB符号文件的加载和缓存。
pub trait SymbolResolver: Send + Sync {
    /// 初始化符号解析器
    fn initialize(&mut self, search_paths: &[PathBuf], cache_dir: &Path) -> Result<(), SymbolError>;
    
    /// 加载模块的符号信息
    fn load_module_symbols(&mut self, module: &ModuleInfo) -> Result<(), SymbolError>;
    
    /// 解析单个地址对应的符号
    fn resolve_address(&self, address: u64) -> Option<SymbolInfo>;
    
    /// 解析调用栈中所有地址的符号
    fn resolve_stack(&self, addresses: &[u64]) -> Vec<Option<SymbolInfo>>;
    
    /// 预加载指定进程的模块符号
    fn preload_process_symbols(&mut self, process_id: u32) -> Result<(), SymbolError>;
    
    /// 获取已加载模块列表
    fn get_loaded_modules(&self) -> Vec<&ModuleInfo>;
    
    /// 清理符号缓存
    fn clear_cache(&mut self);
    
    /// 检查符号是否已加载
    fn is_module_loaded(&self, base_address: u64) -> bool;
}
```

### SampleCollector - 采样收集器

```rust
/// 采样数据收集器
/// 
/// 负责存储和管理采样数据，支持按线程查询。
pub trait SampleCollector: Send + Sync {
    /// 开始收集
    fn start(&mut self) -> Result<(), CollectorError>;
    
    /// 停止收集，返回所有采样数据
    fn stop(&mut self) -> Result<Vec<Sample>, CollectorError>;
    
    /// 添加采样点
    fn add_sample(&mut self, sample: Sample) -> Result<(), CollectorError>;
    
    /// 批量添加采样点
    fn add_samples(&mut self, samples: Vec<Sample>) -> Result<(), CollectorError>;
    
    /// 获取当前采样数量
    fn sample_count(&self) -> u64;
    
    /// 清空所有采样数据
    fn clear(&mut self);
    
    /// 获取指定线程的采样数据
    fn get_thread_samples(&self, thread_id: u32) -> Vec<&Sample>;
    
    /// 获取指定时间范围内的采样
    fn get_samples_in_range(&self, start_ts: u64, end_ts: u64) -> Vec<&Sample>;
}
```

### ThreadTracker - 线程追踪器

```rust
/// 线程追踪器
/// 
/// 负责追踪线程状态和收集线程级别的统计信息。
pub trait ThreadTracker: Send + Sync {
    /// 注册新线程
    fn register_thread(&mut self, thread_id: u32, name: Option<String>, process_id: u32);
    
    /// 注销线程
    fn unregister_thread(&mut self, thread_id: u32);
    
    /// 更新线程状态
    fn update_thread_state(&mut self, thread_id: u32, state: ThreadState);
    
    /// 设置线程名称
    fn set_thread_name(&mut self, thread_id: u32, name: String);
    
    /// 获取线程统计信息
    fn get_thread_stats(&self, thread_id: u32) -> Option<&ThreadStatistics>;
    
    /// 获取所有线程统计
    fn get_all_thread_stats(&self) -> Vec<&ThreadStatistics>;
    
    /// 按采样数据更新统计
    fn update_stats_from_sample(&mut self, sample: &Sample);
    
    /// 获取活跃线程数量
    fn active_thread_count(&self) -> usize;
    
    /// 获取指定进程的线程
    fn get_process_threads(&self, process_id: u32) -> Vec<&ThreadStatistics>;
}
```

### EtwProvider - ETW事件提供者

```rust
/// ETW事件提供者
/// 
/// 负责与Windows ETW API交互，订阅和处理ETW事件。
pub trait EtwProvider: Send + Sync {
    /// 启动ETW会话
    fn start_trace(&mut self, session_name: &str, config: &EtwConfig) -> Result<(), EtwError>;
    
    /// 停止ETW会话
    fn stop_trace(&mut self) -> Result<(), EtwError>;
    
    /// 启用事件提供者
    fn enable_provider(&mut self, provider_id: &Guid, level: u8, keywords: u64) -> Result<(), EtwError>;
    
    /// 禁用事件提供者
    fn disable_provider(&mut self, provider_id: &Guid) -> Result<(), EtwError>;
    
    /// 设置事件回调
    fn set_event_callback(&mut self, callback: Box<dyn Fn(&EtwEvent) -> Result<(), EtwError> + Send>);
    
    /// 处理事件缓冲区
    fn process_events(&mut self, timeout_ms: u32) -> Result<u32, EtwError>;
    
    /// 是否正在运行
    fn is_running(&self) -> bool;
    
    /// 获取当前会话名称
    fn session_name(&self) -> Option<&str>;
}
```

### ProcessMonitor - 进程监控器

```rust
/// 进程监控器
/// 
/// 负责监控进程生命周期事件和模块加载事件。
pub trait ProcessMonitor: Send + Sync {
    /// 启动监控
    fn start_monitoring(&mut self, target: ProcessTarget) -> Result<(), MonitorError>;
    
    /// 停止监控
    fn stop_monitoring(&mut self) -> Result<(), MonitorError>;
    
    /// 设置进程创建回调
    fn on_process_created(&mut self, callback: Box<dyn Fn(u32) + Send>);
    
    /// 设置进程终止回调
    fn on_process_terminated(&mut self, callback: Box<dyn Fn(u32, u32) + Send>);
    
    /// 设置模块加载回调
    fn on_module_loaded(&mut self, callback: Box<dyn Fn(&ModuleInfo) + Send>);
    
    /// 设置模块卸载回调
    fn on_module_unloaded(&mut self, callback: Box<dyn Fn(u64) + Send>);
    
    /// 等待进程退出（阻塞）
    fn wait_for_exit(&self, timeout_ms: u32) -> Result<u32, MonitorError>;
    
    /// 获取目标进程ID
    fn target_process_id(&self) -> Option<u32>;
    
    /// 获取目标进程句柄
    fn target_process_handle(&self) -> Option<usize>;
}
```

### Formatter - 格式化器

```rust
/// 格式化器接口
/// 
/// 用于将报告数据格式化为特定格式（CSV、JSON等）。
pub trait Formatter: Send + Sync {
    /// 格式化报告
    fn format(&self, report: &Report) -> Result<String, FormatError>;
    
    /// 获取文件扩展名
    fn file_extension(&self) -> &'static str;
    
    /// 获取MIME类型
    fn mime_type(&self) -> &'static str;
}

/// CSV格式化器扩展接口
pub trait CsvFormatter: Formatter {
    /// 设置分隔符
    fn set_delimiter(&mut self, delimiter: char);
    
    /// 设置是否包含表头
    fn set_include_headers(&mut self, include: bool);
    
    /// 设置日期时间格式
    fn set_datetime_format(&mut self, format: &str);
}
```

### ReportGenerator - 报告生成器

```rust
/// 报告生成器
/// 
/// 负责将分析数据转换为最终报告。
pub trait ReportGenerator: Send + Sync {
    /// 生成报告
    fn generate(&self, data: &ProfilingData) -> Result<Report, ReportError>;
    
    /// 写入文件
    fn write_to_file(&self, report: &Report, path: &Path) -> Result<(), ReportError>;
    
    /// 设置格式化器
    fn set_formatter(&mut self, formatter: Box<dyn Formatter>);
}
```

### ProfilerLauncher - 分析启动器

```rust
/// 分析启动器
/// 
/// 负责协调Launch和Attach两种分析模式的启动流程。
pub trait ProfilerLauncher: Send + Sync {
    /// 启动分析（Launch模式）
    fn launch(&mut self, config: &ProfilingConfig) -> Result<ProfilingSession, LaunchError>;
    
    /// 附加到现有进程
    fn attach(&mut self, pid: u32, config: &ProfilingConfig) -> Result<ProfilingSession, LaunchError>;
    
    /// 分离会话
    fn detach(&mut self, session: ProfilingSession) -> Result<(), LaunchError>;
    
    /// 等待会话完成
    fn wait_for_completion(&self, session: &ProfilingSession) -> Result<ProfilingData, LaunchError>;
}
```

### ConfigManager - 配置管理器

```rust
/// 配置管理器
/// 
/// 负责加载和验证配置文件。
pub trait ConfigManager: Send + Sync {
    /// 从文件加载配置
    fn load_from_file(&self, path: &Path) -> Result<ProfilingConfig, ProfilerError>;
    
    /// 保存配置到文件
    fn save_to_file(&self, config: &ProfilingConfig, path: &Path) -> Result<(), ProfilerError>;
    
    /// 从命令行参数构建配置
    fn from_args(&self, args: &[String]) -> Result<ProfilingConfig, ProfilerError>;
    
    /// 验证配置有效性
    fn validate(&self, config: &ProfilingConfig) -> Result<(), ProfilerError>;
}
```

## 扩展Trait（高级功能）

```rust
/// 采样过滤器
/// 
/// 用于过滤不需要的采样数据。
pub trait SampleFilter: Send + Sync {
    /// 是否接受该采样
    fn accept(&self, sample: &Sample) -> bool;
    
    /// 获取过滤器描述
    fn description(&self) -> &str;
}

/// 事件过滤器
/// 
/// 用于过滤ETW事件。
pub trait EventFilter: Send + Sync {
    /// 是否接受该事件
    fn accept(&self, event: &EtwEvent) -> bool;
    
    /// 获取过滤器描述
    fn description(&self) -> &str;
}

/// 分析器插件接口
/// 
/// 用于扩展自定义分析功能。
pub trait AnalyzerPlugin: EventHandler {
    /// 获取插件名称
    fn name(&self) -> &str;
    
    /// 获取插件版本
    fn version(&self) -> &str;
    
    /// 初始化插件
    fn initialize(&mut self, config: &ProfilingConfig) -> Result<(), ProfilerError>;
    
    /// 生成插件特定的报告
    fn generate_report(&self) -> Option<Report>;
}
```

## 类型别名

```rust
/// 事件回调函数类型
pub type EventCallback = Box<dyn Fn(&EtwEvent) -> Result<(), EtwError> + Send>;

/// 进程创建回调类型
pub type ProcessCreatedCallback = Box<dyn Fn(u32) + Send>;

/// 进程终止回调类型
pub type ProcessTerminatedCallback = Box<dyn Fn(u32, u32) + Send>;

/// 模块加载回调类型
pub type ModuleLoadedCallback = Box<dyn Fn(&ModuleInfo) + Send>;

/// 模块卸载回调类型
pub type ModuleUnloadedCallback = Box<dyn Fn(u64) + Send>;
```
