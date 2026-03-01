//! 符号解析模块
//!
//! 提供 PDB 符号文件的加载、缓存和解析功能。
//! 使用 Windows DbgHelp API 实现符号解析，支持多进程符号管理。
//!
//! # 主要组件
//!
//! - [`DbgHelpResolver`]: 基于 DbgHelp API 的符号解析器
//! - [`SymbolManager`]: 管理多个进程的符号解析器
//! - [`PdbLocator`]: 自动查找 PDB 文件
//! - [`SymbolCache`]: 符号解析结果缓存
//!
//! # 使用示例
//!
//! ```rust,no_run
//! use profiler::symbols::SymbolManager;
//! use profiler::types::Address;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut manager = SymbolManager::new();
//!
//! // 创建解析器
//! let resolver = manager.create_resolver(1234)?;
//!
//! // 解析地址
//! let address = 0x7ff123456789;
//! if let Some(symbol) = manager.resolve_sample(1234, address)? {
//!     println!("Function: {}", symbol.name);
//! }
//! # Ok(())
//! # }
//! ```

use crate::error::Result;
use crate::types::{Address, ModuleInfo, ProcessId, StackFrame, SymbolInfo};
use std::path::Path;

// 子模块声明
mod cache;
mod loader;
mod manager;
mod resolver;

// 公开导出
pub use cache::{CacheStats, SharedSymbolCache, SymbolCache};
pub use loader::PdbLocator;
pub use manager::{SymbolManager, SymbolManagerStats};
pub use resolver::DbgHelpResolver;

/// 符号解析器 Trait
///
/// 定义符号解析的基本接口，可用于不同的符号解析实现。
pub trait SymbolResolver: Send + Sync {
    /// 解析单个地址
    ///
    /// # 参数
    /// - `address`: 要解析的内存地址
    ///
    /// # 返回
    /// 解析成功的符号信息，失败时返回错误
    fn resolve_address(&self, address: Address) -> Result<SymbolInfo>;

    /// 批量解析堆栈地址
    ///
    /// # 参数
    /// - `addresses`: 地址列表
    ///
    /// # 返回
    /// 解析后的堆栈帧列表
    fn resolve_stack(&self, addresses: &[Address]) -> Result<Vec<StackFrame>>;

    /// 加载模块符号
    ///
    /// # 参数
    /// - `module_path`: 模块文件路径
    /// - `base_address`: 模块基地址
    /// - `module_size`: 模块大小
    fn load_module(
        &mut self,
        module_path: &Path,
        base_address: Address,
        module_size: u32,
    ) -> Result<()>;

    /// 卸载模块符号
    ///
    /// # 参数
    /// - `base_address`: 模块基地址
    fn unload_module(&mut self, base_address: Address) -> Result<()>;

    /// 获取地址所在的模块信息
    ///
    /// # 参数
    /// - `address`: 内存地址
    ///
    /// # 返回
    /// 包含该地址的模块信息
    fn get_module_at_address(&self, address: Address) -> Result<ModuleInfo>;
}

/// 符号加载进度回调
///
/// 用于报告符号加载进度，可用于显示加载状态或取消操作。
pub trait SymbolLoadCallback: Send + Sync {
    /// 报告加载进度
    ///
    /// # 参数
    /// - `module_name`: 当前加载的模块名称
    /// - `current`: 当前进度
    /// - `total`: 总进度
    fn on_progress(&self, module_name: &str, current: usize, total: usize);

    /// 报告加载完成
    ///
    /// # 参数
    /// - `module_name`: 模块名称
    /// - `success`: 是否成功加载
    fn on_complete(&self, module_name: &str, success: bool);

    /// 检查是否取消加载
    ///
    /// # 返回
    /// 如果返回 true，则取消加载操作
    fn is_cancelled(&self) -> bool;
}

/// 默认的符号加载回调（空实现）
pub struct DefaultLoadCallback;

impl SymbolLoadCallback for DefaultLoadCallback {
    fn on_progress(&self, _module_name: &str, _current: usize, _total: usize) {}

    fn on_complete(&self, _module_name: &str, _success: bool) {}

    fn is_cancelled(&self) -> bool {
        false
    }
}

/// 创建符号搜索路径字符串
///
/// 将多个路径组合成 DbgHelp API 需要的格式，支持符号服务器语法。
///
/// # 参数
/// - `paths`: 符号搜索路径列表
/// - `include_system`: 是否包含系统符号路径 (_NT_SYMBOL_PATH)
///
/// # 返回
/// 格式化的搜索路径字符串
///
/// # 示例
///
/// ```rust
/// use profiler::symbols::build_symbol_search_path;
/// use std::path::PathBuf;
///
/// let paths = vec![
///     PathBuf::from("C:\\Symbols"),
///     PathBuf::from("D:\\MySymbols"),
/// ];
///
/// let search_path = build_symbol_search_path(&paths, true);
/// ```
pub fn build_symbol_search_path(paths: &[std::path::PathBuf], include_system: bool) -> String {
    let mut all_paths: Vec<String> = paths
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();

    if include_system {
        if let Ok(sys_path) = std::env::var("_NT_SYMBOL_PATH") {
            if !sys_path.is_empty() {
                // 解析符号服务器语法 srv*Cache*Server
                for part in sys_path.split(';') {
                    all_paths.push(part.to_string());
                }
            }
        }
    }

    all_paths.join(";")
}

/// 从模块路径获取模块名称
///
/// # 参数
/// - `module_path`: 模块文件路径
///
/// # 返回
/// 模块文件名（不含路径）
pub fn get_module_name(module_path: &Path) -> String {
    module_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// 检查地址是否在模块范围内
///
/// # 参数
/// - `address`: 要检查的地址
/// - `base_address`: 模块基地址
/// - `size`: 模块大小
///
/// # 返回
/// 如果地址在模块范围内返回 true
pub fn address_in_module(address: Address, base_address: Address, size: u64) -> bool {
    address >= base_address && address < base_address.saturating_add(size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_build_symbol_search_path() {
        let paths = vec![PathBuf::from("C:\\Symbols"), PathBuf::from("D:\\MySymbols")];

        let search_path = build_symbol_search_path(&paths, false);
        assert!(search_path.contains("C:\\Symbols"));
        assert!(search_path.contains("D:\\MySymbols"));
        assert!(search_path.contains(';'));
    }

    #[test]
    fn test_get_module_name() {
        let path = PathBuf::from("C:\\Windows\\System32\\kernel32.dll");
        assert_eq!(get_module_name(&path), "kernel32.dll");

        let path = PathBuf::from("kernel32.dll");
        assert_eq!(get_module_name(&path), "kernel32.dll");
    }

    #[test]
    fn test_address_in_module() {
        assert!(address_in_module(0x10005000, 0x10000000, 0x10000));
        assert!(!address_in_module(0x10015000, 0x10000000, 0x10000));
        assert!(address_in_module(0x10000000, 0x10000000, 0x10000));
        assert!(!address_in_module(0x10010000, 0x10000000, 0x10000));
    }
}
