//! PDB 文件加载器
//!
//! 自动查找和加载 PDB 符号文件。
//! 支持从多种位置搜索：可执行文件目录、符号搜索路径、
/// 系统符号缓存和符号服务器。

use crate::error::{Result, SymbolError};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, error, info, trace, warn};

/// PDB 定位器
///
/// 负责查找和定位 PDB 符号文件。
/// 支持本地搜索和符号服务器下载。
///
/// # 使用示例
///
/// ```rust,no_run
/// use profiler::symbols::PdbLocator;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let locator = PdbLocator::new()
///     .with_search_paths(vec!["C:\\Symbols".into()])
///     .with_cache_path("C:\\SymbolCache".into());
///
/// let pdb_path = locator.locate_pdb(
///     std::path::Path::new("C:\\Windows\\System32\\kernel32.dll"),
///     None
/// )?;
///
/// if let Some(path) = pdb_path {
///     println!("PDB found: {}", path.display());
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct PdbLocator {
    /// 符号搜索路径列表
    search_paths: Vec<PathBuf>,
    /// 符号服务器 URL（可选）
    symbol_server: Option<String>,
    /// 本地符号缓存路径
    cache_path: Option<PathBuf>,
    /// 是否使用系统符号路径 (_NT_SYMBOL_PATH)
    use_system_path: bool,
    /// 已找到的 PDB 缓存
    found_cache: HashMap<String, Option<PathBuf>>,
}

/// PDB 签名信息
#[derive(Debug, Clone)]
pub struct PdbSignature {
    /// GUID
    pub guid: String,
    /// Age（版本计数）
    pub age: u32,
    /// 原始签名字符串
    pub raw: String,
}

/// 可执行文件中的 PDB 信息
#[derive(Debug, Clone)]
pub struct EmbeddedPdbInfo {
    /// PDB 文件路径（编译时嵌入的）
    pub pdb_path: String,
    /// PDB 签名
    pub signature: Option<PdbSignature>,
}

impl PdbLocator {
    /// 创建新的 PDB 定位器
    pub fn new() -> Self {
        info!("Creating PdbLocator");

        Self {
            search_paths: Vec::new(),
            symbol_server: None,
            cache_path: None,
            use_system_path: true,
            found_cache: HashMap::new(),
        }
    }

    /// 添加搜索路径
    ///
    /// # 参数
    /// - `path`: 要添加的搜索路径
    pub fn with_search_path(mut self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        debug!("Adding PDB search path: {}", path.display());
        self.search_paths.push(path);
        self
    }

    /// 添加多个搜索路径
    ///
    /// # 参数
    /// - `paths`: 搜索路径列表
    pub fn with_search_paths(mut self, paths: Vec<PathBuf>) -> Self {
        debug!("Adding {} PDB search paths", paths.len());
        self.search_paths.extend(paths);
        self
    }

    /// 设置符号服务器
    ///
    /// # 参数
    /// - `server`: 符号服务器 URL，如 "https://msdl.microsoft.com/download/symbols"
    pub fn with_symbol_server(mut self, server: impl Into<String>) -> Self {
        let server = server.into();
        info!("Setting symbol server: {}", server);
        self.symbol_server = Some(server);
        self
    }

    /// 设置本地缓存路径
    ///
    /// # 参数
    /// - `path`: 缓存目录路径
    pub fn with_cache_path(mut self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        info!("Setting symbol cache path: {}", path.display());
        self.cache_path = Some(path);
        self
    }

    /// 设置是否使用系统符号路径
    ///
    /// # 参数
    /// - `use_system`: 是否使用 _NT_SYMBOL_PATH
    pub fn with_system_path(mut self, use_system: bool) -> Self {
        self.use_system_path = use_system;
        debug!("Use system symbol path: {}", use_system);
        self
    }

    /// 查找 PDB 文件
    ///
    /// 尝试在配置的搜索路径中查找 PDB 文件。
    ///
    /// # 参数
    /// - `executable_path`: 可执行文件（或 DLL）的路径
    /// - `pdb_signature`: PDB 签名（可选，用于验证匹配）
    ///
    /// # 返回
    /// 找到的 PDB 文件路径，如果未找到返回 None
    pub fn locate_pdb(
        &self,
        executable_path: &Path,
        pdb_signature: Option<&str>,
    ) -> Result<Option<PathBuf>> {
        let cache_key = format!("{}:{:?}", executable_path.display(), pdb_signature);

        // 检查缓存
        if let Some(cached) = self.found_cache.get(&cache_key) {
            trace!("PDB cache hit for {}", executable_path.display());
            return Ok(cached.clone());
        }

        trace!("Locating PDB for: {}", executable_path.display());

        // 1. 尝试从可执行文件中提取 PDB 路径
        if let Ok(Some(pdb_info)) = self.extract_pdb_info(executable_path) {
            trace!("Embedded PDB path: {}", pdb_info.pdb_path);

            // 尝试使用嵌入的路径查找
            let embedded_path = Path::new(&pdb_info.pdb_path);
            if embedded_path.exists() {
                info!("Found PDB at embedded path: {}", embedded_path.display());
                return Ok(Some(embedded_path.to_path_buf()));
            }

            // 尝试在可执行文件所在目录查找
            if let Some(exe_dir) = executable_path.parent() {
                let pdb_name = Path::new(&pdb_info.pdb_path)
                    .file_name()
                    .unwrap_or_else(|| std::ffi::OsStr::new("unknown.pdb"));
                let local_path = exe_dir.join(pdb_name);

                if local_path.exists() {
                    info!("Found PDB in executable directory: {}", local_path.display());
                    return Ok(Some(local_path));
                }
            }
        }

        // 2. 尝试在可执行文件所在目录查找同名 PDB
        let pdb_name = executable_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string() + ".pdb")
            .unwrap_or_else(|| "unknown.pdb".to_string());

        if let Some(exe_dir) = executable_path.parent() {
            let local_pdb = exe_dir.join(&pdb_name);
            if local_pdb.exists() {
                info!("Found PDB in executable directory: {}", local_pdb.display());
                return Ok(Some(local_pdb));
            }
        }

        // 3. 在配置的搜索路径中查找
        for search_path in &self.search_paths {
            let pdb_path = search_path.join(&pdb_name);
            if pdb_path.exists() {
                info!("Found PDB in search path: {}", pdb_path.display());
                return Ok(Some(pdb_path));
            }

            // 也尝试在子目录中查找（匹配模块名称）
            if let Some(module_name) = executable_path.file_stem() {
                let subdir_path = search_path
                    .join(module_name)
                    .join(&pdb_name);
                if subdir_path.exists() {
                    info!("Found PDB in search path subdir: {}", subdir_path.display());
                    return Ok(Some(subdir_path));
                }
            }
        }

        // 4. 检查系统符号缓存
        if self.use_system_path {
            if let Some(system_pdb) = self.find_in_system_cache(&pdb_name, pdb_signature) {
                info!("Found PDB in system cache: {}", system_pdb.display());
                return Ok(Some(system_pdb));
            }
        }

        // 5. 尝试从符号服务器下载
        if self.symbol_server.is_some() {
            if let Ok(Some(downloaded_pdb)) =
                self.download_from_symbol_server(executable_path, pdb_signature)
            {
                info!("Downloaded PDB from symbol server: {}", downloaded_pdb.display());
                return Ok(Some(downloaded_pdb));
            }
        }

        warn!("PDB not found for: {}", executable_path.display());
        Ok(None)
    }

    /// 获取模块对应的 PDB
    ///
    /// 便捷方法，直接根据模块路径查找 PDB。
    ///
    /// # 参数
    /// - `module_path`: 模块文件路径
    ///
    /// # 返回
    /// 找到的 PDB 路径或 None
    pub fn get_pdb_for_module(&self, module_path: &Path) -> Result<Option<PathBuf>> {
        self.locate_pdb(module_path, None)
    }

    /// 批量查找 PDB
    ///
    /// 为多个模块查找 PDB 文件。
    ///
    /// # 参数
    /// - `modules`: 模块路径列表
    ///
    /// # 返回
    /// 模块路径到 PDB 路径的映射（可能包含 None 表示未找到）
    pub fn locate_pdbs_batch(&self, modules: &[PathBuf]) -> HashMap<PathBuf, Option<PathBuf>> {
        let mut results = HashMap::with_capacity(modules.len());

        for module in modules {
            let result = self.locate_pdb(module, None);
            results.insert(module.clone(), result.ok().flatten());
        }

        results
    }

    /// 解析符号服务器路径
    ///
    /// 解析 _NT_SYMBOL_PATH 格式：srv*Cache*Server
    ///
    /// # 参数
    /// - `symbol_path`: 符号路径字符串
    ///
    /// # 返回
    /// (缓存路径, 服务器URL) 元组
    pub fn parse_symbol_server_path(symbol_path: &str) -> Option<(PathBuf, String)> {
        // 格式: srv*Cache*Server 或 srv*Server
        if !symbol_path.to_lowercase().starts_with("srv*") {
            return None;
        }

        let parts: Vec<&str> = symbol_path[4..].split('*').collect();

        match parts.len() {
            2 => {
                // srv*Cache*Server
                Some((PathBuf::from(parts[0]), parts[1].to_string()))
            }
            1 => {
                // srv*Server (使用默认缓存)
                // 使用系统临时目录作为默认缓存
                let default_cache = std::env::temp_dir().join("Symbols");
                Some((default_cache, parts[0].to_string()))
            }
            _ => None,
        }
    }

    /// 获取所有搜索路径
    pub fn get_search_paths(&self) -> &[PathBuf] {
        &self.search_paths
    }

    /// 获取符号服务器 URL
    pub fn get_symbol_server(&self) -> Option<&str> {
        self.symbol_server.as_deref()
    }

    /// 获取缓存路径
    pub fn get_cache_path(&self) -> Option<&Path> {
        self.cache_path.as_deref()
    }

    /// 添加缓存条目
    pub fn add_to_cache(&mut self, executable_path: &Path, pdb_path: Option<&Path>) {
        let key = executable_path.display().to_string();
        self.found_cache
            .insert(key, pdb_path.map(|p| p.to_path_buf()));
    }

    /// 清除缓存
    pub fn clear_cache(&mut self) {
        self.found_cache.clear();
        debug!("PDB locator cache cleared");
    }

    // 内部辅助方法

    /// 从可执行文件提取 PDB 信息
    fn extract_pdb_info(&self, executable_path: &Path) -> Result<Option<EmbeddedPdbInfo>> {
        // 读取 PE 文件的调试目录
        // 这是一个简化的实现，实际应该使用 goblin 或其他 PE 解析库

        use std::io::{Read, Seek, SeekFrom};

        let mut file = match fs::File::open(executable_path) {
            Ok(f) => f,
            Err(e) => {
                trace!("Cannot open executable: {}", e);
                return Ok(None);
            }
        };

        // 读取 DOS 头
        let mut dos_header = [0u8; 64];
        if file.read_exact(&mut dos_header).is_err() {
            return Ok(None);
        }

        // 获取 PE 头偏移
        let pe_offset = u32::from_le_bytes([dos_header[60], dos_header[61], dos_header[62], dos_header[63]]) as u64;

        // 跳转到 PE 头
        if file.seek(SeekFrom::Start(pe_offset)).is_err() {
            return Ok(None);
        }

        // 读取 PE 签名和 COFF 头
        let mut pe_sig = [0u8; 4];
        if file.read_exact(&mut pe_sig).is_err() || &pe_sig != b"PE\0\0" {
            return Ok(None);
        }

        // 跳过 COFF 头 (20 bytes)
        if file.seek(SeekFrom::Current(20)).is_err() {
            return Ok(None);
        }

        // 读取可选头魔数以确定是 PE32 还是 PE32+
        let mut magic = [0u8; 2];
        if file.read_exact(&mut magic).is_err() {
            return Ok(None);
        }

        // 回退 2 bytes 到可选头开始
        let _ = file.seek(SeekFrom::Current(-2));

        let is_pe32_plus = u16::from_le_bytes(magic) == 0x20b;

        // 跳转到数据目录（第 7 个是调试目录）
        // PE32 可选头大小: 224 bytes, PE32+: 240 bytes
        let data_dir_offset = if is_pe32_plus { 240 - 2 } else { 224 - 2 };
        if file.seek(SeekFrom::Current(data_dir_offset as i64)).is_err() {
            return Ok(None);
        }

        // 读取调试目录 RVA 和大小
        let mut debug_dir_info = [0u8; 8];
        if file.read_exact(&mut debug_dir_info).is_err() {
            return Ok(None);
        }

        let _debug_rva = u32::from_le_bytes([
            debug_dir_info[0],
            debug_dir_info[1],
            debug_dir_info[2],
            debug_dir_info[3],
        ]);
        let _debug_size = u32::from_le_bytes([
            debug_dir_info[4],
            debug_dir_info[5],
            debug_dir_info[6],
            debug_dir_info[7],
        ]);

        // 注意：这里简化处理，实际需要解析节表来转换 RVA 到文件偏移
        // 并读取 DEBUG_DIRECTORY 结构来获取 CodeView 信息

        // 目前返回 None，完整实现需要引入 PE 解析库
        trace!("PE debug directory parsing not fully implemented");
        Ok(None)
    }

    /// 在系统符号缓存中查找
    fn find_in_system_cache(&self, pdb_name: &str, _signature: Option<&str>) -> Option<PathBuf> {
        if !self.use_system_path {
            return None;
        }

        // 获取系统符号路径
        let sys_path = std::env::var("_NT_SYMBOL_PATH").ok()?;

        for part in sys_path.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }

            // 检查是否是符号服务器格式
            if part.to_lowercase().starts_with("srv*") {
                if let Some((cache_path, _)) = Self::parse_symbol_server_path(part) {
                    // 在缓存中查找
                    let cache_pdb = cache_path.join(pdb_name);
                    if cache_pdb.exists() {
                        return Some(cache_pdb);
                    }

                    // 也在 GUID 子目录中查找
                    if let Ok(entries) = fs::read_dir(&cache_path) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.is_dir() {
                                let sub_pdb = path.join(pdb_name);
                                if sub_pdb.exists() {
                                    return Some(sub_pdb);
                                }
                            }
                        }
                    }
                }
            } else {
                // 普通路径
                let path = PathBuf::from(part).join(pdb_name);
                if path.exists() {
                    return Some(path);
                }
            }
        }

        None
    }

    /// 从符号服务器下载 PDB
    fn download_from_symbol_server(
        &self,
        _executable_path: &Path,
        _signature: Option<&str>,
    ) -> Result<Option<PathBuf>> {
        // 注意：实际实现需要 HTTP 客户端和签名验证
        // 这里返回未实现
        trace!("Symbol server download not implemented");
        Ok(None)
    }
}

impl Default for PdbLocator {
    fn default() -> Self {
        Self::new()
    }
}

/// 符号路径构建器
///
/// 用于构建 DbgHelp API 需要的符号搜索路径字符串。
pub struct SymbolPathBuilder {
    paths: Vec<String>,
}

impl SymbolPathBuilder {
    /// 创建新的构建器
    pub fn new() -> Self {
        Self { paths: Vec::new() }
    }

    /// 添加普通路径
    pub fn add_path(mut self, path: impl AsRef<Path>) -> Self {
        self.paths.push(path.as_ref().to_string_lossy().to_string());
        self
    }

    /// 添加符号服务器
    pub fn add_symbol_server(mut self, cache: impl AsRef<Path>, server: impl Into<String>) -> Self {
        let path = format!(
            "srv*{}*{}",
            cache.as_ref().to_string_lossy(),
            server.into()
        );
        self.paths.push(path);
        self
    }

    /// 构建路径字符串
    pub fn build(self) -> String {
        self.paths.join(";")
    }
}

impl Default for SymbolPathBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// 尝试查找 PDB 的便捷函数
///
/// # 参数
/// - `module_path`: 模块路径
/// - `search_paths`: 额外搜索路径
///
/// # 返回
/// 找到的 PDB 路径或 None
pub fn find_pdb(module_path: &Path, search_paths: &[PathBuf]) -> Result<Option<PathBuf>> {
    let locator = PdbLocator::new().with_search_paths(search_paths.to_vec());
    locator.get_pdb_for_module(module_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pdb_locator_creation() {
        let locator = PdbLocator::new();
        assert!(locator.get_search_paths().is_empty());
        assert!(locator.get_symbol_server().is_none());
        assert!(locator.get_cache_path().is_none());
    }

    #[test]
    fn test_pdb_locator_builder() {
        let locator = PdbLocator::new()
            .with_search_path("C:\\Symbols")
            .with_cache_path("C:\\Cache")
            .with_symbol_server("https://example.com/symbols");

        assert_eq!(locator.get_search_paths().len(), 1);
        assert!(locator.get_symbol_server().is_some());
        assert!(locator.get_cache_path().is_some());
    }

    #[test]
    fn test_parse_symbol_server_path() {
        // srv*Cache*Server 格式
        let result = PdbLocator::parse_symbol_server_path("srv*C:\\Cache*https://server.com/symbols");
        assert!(result.is_some());
        let (cache, server) = result.unwrap();
        assert_eq!(cache, PathBuf::from("C:\\Cache"));
        assert_eq!(server, "https://server.com/symbols");

        // srv*Server 格式
        let result = PdbLocator::parse_symbol_server_path("srv*https://server.com/symbols");
        assert!(result.is_some());
        let (_cache, server) = result.unwrap();
        assert_eq!(server, "https://server.com/symbols");

        // 非符号服务器路径
        let result = PdbLocator::parse_symbol_server_path("C:\\Symbols");
        assert!(result.is_none());
    }

    #[test]
    fn test_symbol_path_builder() {
        let path = SymbolPathBuilder::new()
            .add_path("C:\\Symbols")
            .add_symbol_server("C:\\Cache", "https://server.com/symbols")
            .build();

        assert!(path.contains("C:\\Symbols"));
        assert!(path.contains("srv*"));
        assert!(path.contains("https://server.com/symbols"));
    }
}
