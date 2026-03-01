# ETW Windows性能分析工具 - 架构设计文档

本文档目录包含ETW性能分析工具的完整软件架构设计。

## 文档列表

| 文档 | 描述 |
|------|------|
| [architecture.md](./architecture.md) | **核心架构文档**，包含模块划分、职责说明、依赖关系、通信机制、扩展点设计 |
| [traits.md](./traits.md) | **核心Trait和数据结构定义**，所有接口定义和类型定义（Rust代码） |
| [cargo-toml.md](./cargo-toml.md) | **Cargo.toml配置建议**，完整依赖列表和说明 |
| [project-structure.md](./project-structure.md) | **项目目录结构建议**，目录组织方式和代码组织建议 |

## 快速导航

### 1. 了解整体架构
→ 阅读 [architecture.md](./architecture.md)

内容包括：
- 4层架构设计（Application / Core / Infrastructure / Platform）
- 模块依赖关系图
- 事件驱动架构流程
- 多线程模型设计
- 扩展点设计

### 2. 查看接口定义
→ 阅读 [traits.md](./traits.md)

内容包括：
- 13个核心Trait定义
- 20+个数据结构定义
- 错误类型定义
- 类型别名

### 3. 配置项目依赖
→ 阅读 [cargo-toml.md](./cargo-toml.md)

内容包括：
- 完整Cargo.toml配置
- Windows API依赖说明
- 可选依赖建议
- 最小化配置版本

### 4. 组织项目代码
→ 阅读 [project-structure.md](./project-structure.md)

内容包括：
- 单crate结构（推荐初期）
- 多crate工作区结构（大型项目）
- 模块依赖关系
- 代码组织建议

## 架构概览

```
┌─────────────────────────────────────────────────────────────┐
│                    Application Layer                         │
│         CLI / Config / Report Generator / Launcher          │
└─────────────────────────────┬───────────────────────────────┘
                              │
┌─────────────────────────────▼───────────────────────────────┐
│                     Core Service Layer                       │
│      Session Manager / Event Processor / Sample Collector   │
│                     Thread Tracker                           │
└─────────────────────────────┬───────────────────────────────┘
                              │
┌─────────────────────────────▼───────────────────────────────┐
│                   Infrastructure Layer                       │
│      ETW Provider / Symbol Resolver / Process Monitor       │
│                      Output Formatter                        │
└─────────────────────────────┬───────────────────────────────┘
                              │
┌─────────────────────────────▼───────────────────────────────┐
│                   Platform Abstraction                       │
│           Windows ETW API / DbgHelp API / Win32 API         │
└─────────────────────────────────────────────────────────────┘
```

## 核心Trait列表

| Trait | 职责 | 所在层级 |
|-------|------|----------|
| `SessionManager` | 会话生命周期管理 | Core |
| `EventHandler` | 事件处理回调 | Core |
| `EventHandlerRegistry` | 事件处理器管理 | Core |
| `SampleCollector` | 采样数据收集 | Core |
| `ThreadTracker` | 线程状态追踪 | Core |
| `SymbolResolver` | 符号解析 | Infrastructure |
| `EtwProvider` | ETW事件提供 | Infrastructure |
| `ProcessMonitor` | 进程监控 | Infrastructure |
| `Formatter` | 输出格式化 | Infrastructure |
| `ReportGenerator` | 报告生成 | Application |
| `ProfilerLauncher` | 启动/附加协调 | Application |
| `ConfigManager` | 配置管理 | Application |

## 关键设计决策

### 1. 分层架构
- **Application Layer**: 用户交互、配置管理、报告生成
- **Core Layer**: 核心业务逻辑、事件处理、数据收集
- **Infrastructure Layer**: Windows API封装、符号解析、ETW交互
- **Platform Layer**: Windows原生API

### 2. 通信机制
- **事件驱动**: ETW Provider → Event Processor → Handler Chain
- **多线程**: ETW Consumer Thread + Event Processing Thread + Worker Pool

### 3. 扩展性设计
- **Trait-based**: 通过实现Trait扩展功能
- **插件接口**: `AnalyzerPlugin` trait支持自定义分析器
- **格式扩展**: `Formatter` trait支持多种输出格式

## 下一步工作

实现阶段建议按以下顺序进行：

1. **基础结构** (Week 1)
   - 创建项目结构
   - 定义错误类型
   - 实现基础数据结构

2. **ETW基础设施** (Week 2)
   - 实现 `EtwProvider`
   - 事件解析器
   - 基础事件循环

3. **符号解析** (Week 2-3)
   - 实现 `SymbolResolver`
   - PDB加载
   - 地址解析

4. **核心服务** (Week 3-4)
   - 实现 `SessionManager`
   - 实现 `SampleCollector`
   - 实现 `ThreadTracker`

5. **应用层** (Week 4)
   - CLI实现
   - 配置管理
   - CSV报告生成

6. **集成测试** (Week 5)
   - Launch模式测试
   - Attach模式测试
   - 端到端测试

## 参考资源

- [Windows ETW Documentation](https://docs.microsoft.com/en-us/windows/win32/etw/event-tracing-portal)
- [windows-rs crate](https://github.com/microsoft/windows-rs)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
