# Cargo.toml 配置建议

## 主项目配置

```toml
[package]
name = "etw-profiler"
version = "0.1.0"
edition = "2024"
authors = ["Your Name <your.email@example.com>"]
description = "A Windows performance profiler based on ETW"
license = "MIT OR Apache-2.0"
repository = "https://github.com/yourusername/etw-profiler"
keywords = ["profiler", "etw", "performance", "windows", "sampling"]
categories = ["development-tools::profiling", "os::windows-apis"]
rust-version = "1.80"

[[bin]]
name = "etw-profiler"
path = "src/main.rs"

[dependencies]
# ============================================================================
# 核心依赖
# ============================================================================

# Windows API绑定 - 官方Rust Windows绑定
# 用于调用ETW API、DbgHelp API等Windows原生API
windows = { version = "0.58", features = [
    "Win32_Foundation",
    "Win32_System_Diagnostics_Etw",
    "Win32_System_Diagnostics_Debug",
    "Win32_System_Threading",
    "Win32_System_ProcessStatus",
    "Win32_System_LibraryLoader",
    "Win32_System_Time",
    "Win32_Security",
    "Win32_Storage_FileSystem",
    "Win32_System_WindowsProgramming",
    "Win32_System_SystemInformation",
    "Win32_System_Diagnostics_Debug_ActiveScript",
    "Win32_System_Kernel",
] }

# Windows核心库
windows-core = "0.58"

# 可选：更底层的Windows API绑定（如果需要更细粒度的控制）
# windows-sys = "0.52"

# ============================================================================
# 序列化/反序列化
# ============================================================================

# 用于配置文件解析和JSON输出格式
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# TOML配置文件支持
toml = "0.8"

# ============================================================================
# 异步运行时（如果需要）
# ============================================================================

# 异步运行时，用于ETW事件处理
tokio = { version = "1.40", features = [
    "rt-multi-thread",
    "sync",
    "time",
    "macros",
] }

# 跨线程消息传递
# crossbeam-channel = "0.5"

# ============================================================================
# 日志与错误处理
# ============================================================================

# 日志框架
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }

# 错误处理
thiserror = "1.0"
anyhow = "1.0"

# ============================================================================
# 命令行参数解析
# ============================================================================

clap = { version = "4.5", features = ["derive", "cargo", "env"] }

# ============================================================================
# 数据处理
# ============================================================================

# CSV输出格式
csv = "1.3"

# 高性能哈希表
# rustc-hash = "1.1"

# 有序集合/映射（用于排序输出）
# indexmap = "2.4"

# ============================================================================
# 并发与同步
# ============================================================================

# 读写锁优化
# parking_lot = "0.12"

# 并发数据结构
# dashmap = "6.0"

# 无锁数据结构
# crossbeam = "0.8"

# ============================================================================
# 工具库
# ============================================================================

# 路径处理工具
# camino = "1.1"

# 命令行进度条
# indicatif = "0.17"

# 控制台颜色输出
# console = "0.15"

# 人类可读格式（字节、时间等）
# humansize = "2.1"
# humantime = "2.1"

# UUID生成
uuid = { version = "1.10", features = ["v4", "serde"] }

# 正则表达式（如果需要模式匹配）
# regex = "1.10"

# ============================================================================
# 可选依赖
# ============================================================================

[dependencies.chrono]
version = "0.4"
optional = true

[dependencies.sqlx]
version = "0.8"
optional = true
features = ["runtime-tokio", "sqlite"]

# ============================================================================
# 特性开关
# ============================================================================

[features]
default = ["chrono"]

# 启用数据库存储后端
database = ["sqlx"]

# 启用高级分析功能（火焰图、调用图等）
advanced = ["chrono"]

# 开发调试功能
dev = ["tracing/max_level_debug"]

# ============================================================================
# 开发依赖
# ============================================================================

[dev-dependencies]
# 测试框架
tokio-test = "0.4"

# 模拟/存根库
# mockall = "0.13"

# 临时文件/目录
tempfile = "3.12"

# 断言增强
# pretty_assertions = "1.4"

# 基准测试
criterion = { version = "0.5", features = ["html_reports"] }

# 模糊测试（可选）
# libfuzzer-sys = "0.4"

# ============================================================================
# 构建配置
# ============================================================================

[profile.release]
# 优化级别
opt-level = 3
# 链接时优化
lto = "thin"
# 代码生成单元（更小的单元 = 更好的优化）
codegen-units = 1
#  panic处理
panic = "abort"
# 调试信息（用于符号化堆栈跟踪）
debug = true
#  strip符号
strip = false

[profile.release.build-override]
opt-level = 3

[profile.dev]
# 开发配置
opt-level = 0
debug = true

# ============================================================================
# 构建脚本
# ============================================================================

# 如果需要自定义构建逻辑（如编译C代码）
# [package]
# build = "build.rs"

# build-dependencies
# [build-dependencies]
# cc = "1.0"

# ============================================================================
# 工作区配置（多crate项目）
# ============================================================================

# [workspace]
# members = [
#     "crates/etw-core",
#     "crates/etw-symbols",
#     "crates/etw-cli",
#     "crates/etw-reports",
# ]
# resolver = "2"

# [workspace.dependencies]
# windows = "0.58"
# tokio = "1.40"
# serde = "1.0"
```

## 建议的依赖选择说明

| 依赖 | 必要性 | 用途 |
|------|--------|------|
| `windows` | **必需** | Windows API绑定，核心依赖 |
| `serde` + `toml` | **必需** | 配置文件解析 |
| `tokio` | **推荐** | 异步运行时，ETW事件处理需要 |
| `clap` | **必需** | 命令行参数解析 |
| `tracing` | **推荐** | 结构化日志 |
| `thiserror` + `anyhow` | **必需** | 错误处理 |
| `csv` | **必需** | CSV输出格式 |
| `uuid` | **推荐** | 会话ID生成 |

## 最小化Cargo.toml（快速开始）

如果希望最小化依赖，可以使用以下配置：

```toml
[package]
name = "etw-profiler"
version = "0.1.0"
edition = "2024"

[dependencies]
windows = { version = "0.58", features = [
    "Win32_Foundation",
    "Win32_System_Diagnostics_Etw",
    "Win32_System_Diagnostics_Debug",
    "Win32_System_Threading",
] }
clap = { version = "4.5", features = ["derive"] }
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"
csv = "1.3"
thiserror = "1.0"
```
