# 项目目录结构建议

## 单crate项目结构（推荐用于初期开发）

```
etw-profiler/
├── Cargo.toml                    # 项目配置
├── Cargo.lock                    # 依赖锁定
├── README.md                     # 项目说明
├── LICENSE                       # 许可证
├── .gitignore                    # Git忽略配置
├── rustfmt.toml                  # 代码格式化配置（可选）
├── clippy.toml                   # Clippy配置（可选）
│
├── src/
│   ├── main.rs                   # 程序入口
│   ├── lib.rs                    # 库入口（可选，如果同时提供库）
│   │
│   ├── core/                     # 核心服务层
│   │   ├── mod.rs
│   │   ├── session.rs            # SessionManager trait + 实现
│   │   ├── event_processor.rs    # EventHandler + EventHandlerRegistry
│   │   ├── sample_collector.rs   # SampleCollector trait + 实现
│   │   └── thread_tracker.rs     # ThreadTracker trait + 实现
│   │
│   ├── infrastructure/           # 基础设施层
│   │   ├── mod.rs
│   │   ├── etw/                  # ETW提供者模块
│   │   │   ├── mod.rs
│   │   │   ├── provider.rs       # EtwProvider实现
│   │   │   ├── event_parser.rs   # ETW事件解析
│   │   │   ├── constants.rs      # ETW GUID常量
│   │   │   └── ffi.rs            # FFI绑定（如果需要）
│   │   │
│   │   ├── symbols/              # 符号解析模块
│   │   │   ├── mod.rs
│   │   │   ├── resolver.rs       # SymbolResolver实现
│   │   │   ├── pdb_loader.rs     # PDB加载器
│   │   │   ├── cache.rs          # 符号缓存管理
│   │   │   └── ffi.rs            # DbgHelp FFI
│   │   │
│   │   ├── process/              # 进程监控模块
│   │   │   ├── mod.rs
│   │   │   ├── monitor.rs        # ProcessMonitor实现
│   │   │   ├── launcher.rs       # Launch/Attach逻辑
│   │   │   └── module_tracker.rs # 模块加载追踪
│   │   │
│   │   └── output/               # 输出格式化模块
│   │       ├── mod.rs
│   │       ├── formatter.rs      # Formatter trait
│   │       ├── csv_formatter.rs  # CSV格式实现
│   │       └── report_gen.rs     # ReportGenerator实现
│   │
│   ├── application/              # 应用层
│   │   ├── mod.rs
│   │   ├── cli/                  # CLI模块
│   │   │   ├── mod.rs
│   │   │   ├── args.rs           # 命令行参数定义
│   │   │   ├── commands.rs       # 命令处理
│   │   │   └── ui.rs             # 用户界面/进度显示
│   │   │
│   │   ├── config/               # 配置管理
│   │   │   ├── mod.rs
│   │   │   ├── loader.rs         # ConfigManager实现
│   │   │   └── model.rs          # 配置数据结构
│   │   │
│   │   └── orchestrator.rs       # Launch Orchestrator
│   │
│   ├── domain/                   # 领域模型（纯数据结构）
│   │   ├── mod.rs
│   │   ├── types.rs              # 基础类型定义
│   │   ├── sample.rs             # Sample, StackTrace等
│   │   ├── symbol.rs             # SymbolInfo, ModuleInfo等
│   │   ├── thread.rs             # ThreadStatistics等
│   │   ├── events.rs             # EtwEvent等
│   │   └── config.rs             # ProfilingConfig等
│   │
│   ├── traits/                   # Trait定义集中存放（可选）
│   │   ├── mod.rs
│   │   ├── session.rs
│   │   ├── event.rs
│   │   ├── symbol.rs
│   │   ├── collector.rs
│   │   ├── tracker.rs
│   │   ├── etw.rs
│   │   ├── process.rs
│   │   ├── output.rs
│   │   └── launcher.rs
│   │
│   └── utils/                    # 工具模块
│       ├── mod.rs
│       ├── error.rs              # 错误类型定义
│       ├── time.rs               # 时间处理工具
│       ├── path.rs               # 路径处理工具
│       └── sync.rs               # 同步原语工具
│
├── tests/                        # 集成测试
│   ├── integration_tests.rs
│   ├── fixtures/                 # 测试数据
│   │   ├── sample_config.toml
│   │   └── test_program.exe
│   └── helpers/                  # 测试辅助函数
│       └── mod.rs
│
├── benches/                      # 基准测试
│   └── sample_processing.rs
│
├── examples/                     # 使用示例
│   ├── basic_profiling.rs
│   ├── custom_handler.rs
│   └── json_output.rs
│
└── docs/                         # 额外文档
    ├── ARCHITECTURE.md           # 架构说明
    ├── API.md                    # API文档
    └── USAGE.md                  # 使用指南
```

## 多crate工作区结构（推荐用于大型项目）

```
etw-profiler/
├── Cargo.toml                    # 工作区配置
├── Cargo.lock
├── README.md
├── LICENSE
├── .gitignore
│
├── crates/
│   ├── etw-core/                 # 核心领域模型和trait
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── types/
│   │       │   ├── mod.rs
│   │       │   ├── sample.rs
│   │       │   ├── symbol.rs
│   │       │   ├── thread.rs
│   │       │   └── events.rs
│   │       └── traits/
│   │           ├── mod.rs
│   │           ├── session.rs
│   │           ├── event.rs
│   │           ├── symbol.rs
│   │           ├── collector.rs
│   │           └── tracker.rs
│   │
│   ├── etw-infrastructure/       # 基础设施实现
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── etw/
│   │       │   ├── mod.rs
│   │       │   └── provider.rs
│   │       ├── symbols/
│   │       │   ├── mod.rs
│   │       │   └── resolver.rs
│   │       ├── process/
│   │       │   ├── mod.rs
│   │       │   └── monitor.rs
│   │       └── output/
│   │           ├── mod.rs
│   │           └── formatters/
│   │
│   ├── etw-application/          # 应用层
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── cli/
│   │       ├── config/
│   │       └── orchestrator.rs
│   │
│   └── etw-cli/                  # 可执行程序
│       ├── Cargo.toml
│       └── src/
│           └── main.rs
│
├── tests/                        # 集成测试（跨crate）
├── benches/                      # 基准测试
├── examples/                     # 示例代码
└── docs/                         # 文档
```

## 推荐的初始项目结构（最小化）

对于快速启动，建议从以下简化结构开始：

```
etw-profiler/
├── Cargo.toml
├── README.md
├── .gitignore
│
└── src/
    ├── main.rs                   # CLI入口和参数解析
    ├── lib.rs                    # 库模块组织
    │
    ├── error.rs                  # 错误类型定义
    ├── types.rs                  # 核心数据结构
    ├── traits.rs                 # 所有trait定义
    │
    ├── session.rs                # SessionManager
    ├── etw_provider.rs           # EtwProvider实现
    ├── symbol_resolver.rs        # SymbolResolver实现
    ├── sample_collector.rs       # SampleCollector实现
    ├── thread_tracker.rs         # ThreadTracker实现
    ├── process_monitor.rs        # ProcessMonitor实现
    ├── report_generator.rs       # ReportGenerator实现
    │
    └── utils.rs                  # 工具函数
```

## 模块依赖关系图

```
┌────────────────────────────────────────────────────────────────┐
│                         main.rs                                 │
│                   (应用程序入口)                                 │
└──────────────────────────┬─────────────────────────────────────┘
                           │
           ┌───────────────┼───────────────┐
           ▼               ▼               ▼
┌─────────────────┐ ┌─────────────┐ ┌──────────────────┐
│   session.rs    │ │   utils.rs  │ │  error.rs        │
│ (SessionManager)│ │ (工具函数)   │ │ (错误定义)       │
└────────┬────────┘ └─────────────┘ └──────────────────┘
         │
    ┌────┴────┬────────────┬────────────┐
    ▼         ▼            ▼            ▼
┌────────┐ ┌──────────┐ ┌──────────┐ ┌──────────────┐
│ etw_   │ │ symbol_  │ │ sample_  │ │ process_     │
│provider│ │resolver  │ │collector │ │monitor       │
│        │ │          │ │          │ │              │
│EtwProv-│ │SymbolRes-│ │SampleCol-│ │ProcessMonitor│
│ ider   │ │ olver    │ │ lector    │ │              │
└────────┘ └──────────┘ └──────────┘ └──────────────┘
    │            │            │             │
    └────────────┴────────────┴─────────────┘
                   │
                   ▼
           ┌───────────────┐
           │  report_      │
           │  generator    │
           │               │
           │ ReportGenerator
           └───────────────┘
```

## 代码组织建议

### 1. 模块可见性

```rust
// lib.rs
pub mod error;        // 公开错误类型
pub mod types;        // 公开数据结构
pub mod traits;       // 公开trait定义

// 实现模块可以设为crate私有
pub(crate) mod session;
pub(crate) mod etw_provider;
pub(crate) mod symbol_resolver;
// ...
```

### 2. Trait组织方式

方式A：集中式（推荐用于小型项目）
```rust
// traits.rs
pub trait SessionManager { /* ... */ }
pub trait EventHandler { /* ... */ }
pub trait SymbolResolver { /* ... */ }
// ...
```

方式B：分布式（推荐用于大型项目）
```rust
// traits/mod.rs
pub mod session;
pub mod event;
pub mod symbol;
// ...

// traits/session.rs
pub trait SessionManager { /* ... */ }
```

### 3. 错误处理组织

```rust
// error.rs
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProfilerError {
    #[error("Session error: {0}")]
    Session(#[from] SessionError),
    
    #[error("Symbol error: {0}")]
    Symbol(#[from] SymbolError),
    
    #[error("ETW error: {0}")]
    Etw(#[from] EtwError),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Error, Debug)]
#[error("{0}")]
pub struct SessionError(pub String);

// ...
```

### 4. Windows API Feature组织

```rust
// etw_provider.rs
use windows::Win32::System::Diagnostics::Etw::*;
use windows::Win32::Foundation::*;

pub struct WindowsEtwProvider {
    session_handle: CONTROLTRACE_HANDLE,
    // ...
}

impl EtwProvider for WindowsEtwProvider {
    // 实现...
}
```

## .gitignore 建议

```gitignore
# Rust
/target
**/*.rs.bk
Cargo.lock

# IDE
.idea/
.vscode/
*.swp
*.swo
*~

# OS
.DS_Store
Thumbs.db

# 项目特定
*.csv
*.etl
*.pdb
/symbol_cache/
/profile_reports/
*.log

# 测试输出
/test_output/
```

## rustfmt.toml 建议

```toml
edition = "2024"
max_width = 100
tab_spaces = 4
use_small_heuristics = "Default"
reorder_imports = true
reorder_modules = true
remove_nested_parens = true
newline_style = "Unix"
```

## 目录结构选择指南

| 场景 | 推荐结构 |
|------|----------|
| 个人项目/原型 | 单文件或最小化结构 |
| 小型团队项目 | 单crate分层结构 |
| 大型项目/多团队协作 | 多crate工作区结构 |
| 需要作为库发布 | 多crate（core + impl分离） |

## 文件命名约定

- **模块文件**: `snake_case.rs`
- **Trait定义**: 与概念同名，如 `session.rs` 包含 `SessionManager` trait
- **实现文件**: 如果trait和实现分离，使用 `trait_name_impl.rs`
- **测试文件**: `#[cfg(test)]` 放在被测试文件中，集成测试放在 `tests/` 目录
