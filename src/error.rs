//! 错误类型定义模块
//!
//! 提供统一的错误类型 `ProfilerError`，用于整个性能分析工具的
//! 错误处理和传播。使用 `thiserror` crate 简化错误定义。

use thiserror::Error;

/// 性能分析器统一错误类型
///
/// 所有模块的错误都会被封装在此枚举中，便于上层统一处理。
/// 实现了 `std::error::Error` 和 `std::fmt::Display` trait。
#[derive(Error, Debug)]
pub enum ProfilerError {
    /// ETW 相关错误
    ///
    /// 包括会话启动失败、事件订阅失败、缓冲区处理错误等。
    #[error("ETW error: {0}")]
    EtwError(#[from] EtwError),

    /// 符号解析错误
    ///
    /// 包括 PDB 加载失败、符号查找失败、模块解析错误等。
    #[error("Symbol error: {0}")]
    SymbolError(#[from] SymbolError),

    /// IO 错误
    ///
    /// 包括文件读写、路径操作、网络访问等错误。
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// 配置错误
    ///
    /// 包括配置文件解析失败、配置项缺失或无效等。
    #[error("Configuration error: {0}")]
    ConfigError(#[from] ConfigError),

    /// 进程操作错误
    ///
    /// 包括进程启动失败、附加失败、权限不足等。
    #[error("Process error: {0}")]
    ProcessError(#[from] ProcessError),

    /// 通用错误
    ///
    /// 用于其他无法归类的错误情况。
    #[error("{0}")]
    Generic(String),
}

/// ETW 错误类型
///
/// 封装 Windows ETW API 调用过程中可能发生的各种错误。
#[derive(Error, Debug, Clone)]
#[error("{message}")]
pub struct EtwError {
    /// 错误描述信息
    pub message: String,
    /// 可选的错误代码
    pub code: Option<u32>,
}

impl EtwError {
    /// 创建新的 ETW 错误
    ///
    /// # 参数
    /// - `message`: 错误描述信息
    ///
    /// # 示例
    /// ```
    /// let err = EtwError::new("Failed to start ETW session");
    /// ```
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: None,
        }
    }

    /// 创建带错误代码的 ETW 错误
    ///
    /// # 参数
    /// - `message`: 错误描述信息
    /// - `code`: Windows 错误代码
    pub fn with_code(message: impl Into<String>, code: u32) -> Self {
        Self {
            message: message.into(),
            code: Some(code),
        }
    }
}

/// 符号解析错误类型
///
/// 封装符号解析和 PDB 加载过程中的各种错误。
#[derive(Error, Debug, Clone)]
#[error("{message}")]
pub struct SymbolError {
    /// 错误描述信息
    pub message: String,
    /// 相关地址（如果适用）
    pub address: Option<u64>,
    /// 模块名称（如果适用）
    pub module: Option<String>,
}

impl SymbolError {
    /// 创建新的符号错误
    ///
    /// # 参数
    /// - `message`: 错误描述信息
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            address: None,
            module: None,
        }
    }

    /// 创建带地址的符号错误
    ///
    /// # 参数
    /// - `message`: 错误描述信息
    /// - `address`: 相关内存地址
    pub fn with_address(message: impl Into<String>, address: u64) -> Self {
        Self {
            message: message.into(),
            address: Some(address),
            module: None,
        }
    }

    /// 创建带模块信息的符号错误
    ///
    /// # 参数
    /// - `message`: 错误描述信息
    /// - `module`: 模块名称
    pub fn with_module(message: impl Into<String>, module: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            address: None,
            module: Some(module.into()),
        }
    }
}

/// 配置错误类型
///
/// 封装配置文件解析和验证过程中的各种错误。
#[derive(Error, Debug, Clone)]
#[error("{message}")]
pub struct ConfigError {
    /// 错误描述信息
    pub message: String,
    /// 相关配置项名称
    pub field: Option<String>,
    /// 配置文件路径
    pub file: Option<String>,
}

impl ConfigError {
    /// 创建新的配置错误
    ///
    /// # 参数
    /// - `message`: 错误描述信息
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            field: None,
            file: None,
        }
    }

    /// 创建带字段信息的配置错误
    ///
    /// # 参数
    /// - `message`: 错误描述信息
    /// - `field`: 相关配置项名称
    pub fn with_field(message: impl Into<String>, field: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            field: Some(field.into()),
            file: None,
        }
    }

    /// 创建带文件信息的配置错误
    ///
    /// # 参数
    /// - `message`: 错误描述信息
    /// - `file`: 配置文件路径
    /// - `field`: 相关配置项名称（可选）
    pub fn with_file(
        message: impl Into<String>,
        file: impl Into<String>,
        field: Option<String>,
    ) -> Self {
        Self {
            message: message.into(),
            field,
            file: Some(file.into()),
        }
    }
}

/// 进程操作错误类型
///
/// 封装进程启动、附加和监控过程中的各种错误。
#[derive(Error, Debug, Clone)]
#[error("{message}")]
pub struct ProcessError {
    /// 错误描述信息
    pub message: String,
    /// 进程 ID（如果适用）
    pub pid: Option<u32>,
    /// 进程名称（如果适用）
    pub name: Option<String>,
}

impl ProcessError {
    /// 创建新的进程错误
    ///
    /// # 参数
    /// - `message`: 错误描述信息
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            pid: None,
            name: None,
        }
    }

    /// 创建带进程 ID 的进程错误
    ///
    /// # 参数
    /// - `message`: 错误描述信息
    /// - `pid`: 进程 ID
    pub fn with_pid(message: impl Into<String>, pid: u32) -> Self {
        Self {
            message: message.into(),
            pid: Some(pid),
            name: None,
        }
    }

    /// 创建带进程名称的进程错误
    ///
    /// # 参数
    /// - `message`: 错误描述信息
    /// - `name`: 进程名称
    pub fn with_name(message: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            pid: None,
            name: Some(name.into()),
        }
    }
}

/// 便捷类型别名：性能分析器结果类型
///
/// 用于统一函数返回结果类型，简化错误处理。
pub type Result<T> = std::result::Result<T, ProfilerError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_etw_error_new() {
        let err = EtwError::new("test error");
        assert_eq!(err.message, "test error");
        assert_eq!(err.code, None);
    }

    #[test]
    fn test_etw_error_with_code() {
        let err = EtwError::with_code("test error", 123);
        assert_eq!(err.message, "test error");
        assert_eq!(err.code, Some(123));
    }

    #[test]
    fn test_symbol_error_new() {
        let err = SymbolError::new("symbol not found");
        assert_eq!(err.message, "symbol not found");
        assert_eq!(err.address, None);
    }

    #[test]
    fn test_symbol_error_with_address() {
        let err = SymbolError::with_address("symbol not found", 0x12345678);
        assert_eq!(err.address, Some(0x12345678));
    }

    #[test]
    fn test_config_error_new() {
        let err = ConfigError::new("invalid config");
        assert_eq!(err.message, "invalid config");
    }

    #[test]
    fn test_process_error_with_pid() {
        let err = ProcessError::with_pid("access denied", 1234);
        assert_eq!(err.pid, Some(1234));
    }

    #[test]
    fn test_profiler_error_display() {
        let etw_err = EtwError::new("test");
        let profiler_err = ProfilerError::from(etw_err);
        let display = format!("{}", profiler_err);
        assert!(display.contains("ETW error"));
        assert!(display.contains("test"));
    }
}
