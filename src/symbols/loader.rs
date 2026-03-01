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

/// PE 节表信息
#[derive(Debug, Clone)]
struct SectionInfo {
    /// 虚拟地址
    virtual_address: u32,
    /// 虚拟大小
    virtual_size: u32,
    /// 文件偏移
    pointer_to_raw_data: u32,
    /// 原始数据大小
    size_of_raw_data: u32,
}

/// 将 RVA（相对虚拟地址）转换为文件偏移
fn rva_to_file_offset(rva: u32, sections: &[SectionInfo]) -> u32 {
    for section in sections {
        // 检查 RVA 是否在此节的范围内
        if rva >= section.virtual_address &&
           rva < section.virtual_address + section.virtual_size {
            // 计算偏移量
            let offset_in_section = rva - section.virtual_address;
            // 确保不超过原始数据大小
            if offset_in_section < section.size_of_raw_data {
                return section.pointer_to_raw_data + offset_in_section;
            }
        }
    }
    // 如果没有匹配的节，可能是 RVA 已经在文件偏移范围内
    rva
}

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
        } else {
            trace!("Could not extract PDB info from executable: {}", executable_path.display());
            trace!("extract_pdb_info is not fully implemented - see loader.rs line 342");
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
    ///
    /// 解析 PE 文件的 Debug Directory，读取 CodeView 信息（NB10 或 RSDS 格式），
    /// 提取 PDB 路径和签名信息。
    ///
    /// # 参数
    /// - `executable_path`: 可执行文件（PE）的路径
    ///
    /// # 返回
    /// 包含 PDB 路径和签名的信息，如果 PE 文件不包含 CodeView 信息则返回 None
    pub fn extract_pdb_info(&self, executable_path: &Path) -> Result<Option<EmbeddedPdbInfo>> {
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

        // 验证 DOS 签名
        if dos_header[0] != 0x4D || dos_header[1] != 0x5A {
            trace!("Invalid DOS signature");
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

        // 读取 COFF 头
        let mut coff_header = [0u8; 20];
        if file.read_exact(&mut coff_header).is_err() {
            return Ok(None);
        }

        // 提取 COFF 头信息
        let number_of_sections = u16::from_le_bytes([coff_header[2], coff_header[3]]);
        let size_of_optional_header = u16::from_le_bytes([coff_header[16], coff_header[17]]);

        if size_of_optional_header == 0 {
            trace!("No optional header in PE file");
            return Ok(None);
        }

        // 读取可选头魔数以确定是 PE32 还是 PE32+
        let mut magic = [0u8; 2];
        if file.read_exact(&mut magic).is_err() {
            return Ok(None);
        }

        let is_pe32_plus = u16::from_le_bytes(magic) == 0x20b;

        trace!("PE file type: {}", if is_pe32_plus { "PE32+" } else { "PE32" });

        // 回到可选头开始位置（PE 签名后 4 bytes + COFF 头 20 bytes = 24 bytes from PE start）
        if file.seek(SeekFrom::Start(pe_offset + 24)).is_err() {
            return Ok(None);
        }

        // 读取完整的可选头
        let optional_header_size = if is_pe32_plus { 240 } else { 224 };
        let mut optional_header = vec![0u8; optional_header_size];
        if file.read_exact(&mut optional_header).is_err() {
            return Ok(None);
        }

        // 数据目录在可选头的最后 128 bytes (16 entries * 8 bytes each)
        // PE32: 数据目录从 offset 96 开始
        // PE32+: 数据目录从 offset 112 开始
        let data_dir_offset = if is_pe32_plus { 112 } else { 96 };

        // 调试目录是第7个条目 (索引 6)，每个条目 8 bytes
        let debug_dir_entry_offset = data_dir_offset + (6 * 8);

        let debug_rva = u32::from_le_bytes([
            optional_header[debug_dir_entry_offset],
            optional_header[debug_dir_entry_offset + 1],
            optional_header[debug_dir_entry_offset + 2],
            optional_header[debug_dir_entry_offset + 3],
        ]);
        let debug_size = u32::from_le_bytes([
            optional_header[debug_dir_entry_offset + 4],
            optional_header[debug_dir_entry_offset + 5],
            optional_header[debug_dir_entry_offset + 6],
            optional_header[debug_dir_entry_offset + 7],
        ]);

        if debug_rva == 0 || debug_size == 0 {
            trace!("No debug directory in PE file");
            return Ok(None);
        }

        trace!("Debug directory RVA: 0x{:08X}, Size: {}", debug_rva, debug_size);

        // 读取节表以转换 RVA 到文件偏移
        // 节表在可选头之后
        if file.seek(SeekFrom::Start(pe_offset + 24 + optional_header_size as u64)).is_err() {
            return Ok(None);
        }

        // 现在读取节表
        let mut sections = Vec::new();
        for i in 0..number_of_sections {
            let mut section_header = [0u8; 40];
            if file.read_exact(&mut section_header).is_err() {
                break;
            }

            let virtual_address = u32::from_le_bytes([
                section_header[12], section_header[13],
                section_header[14], section_header[15],
            ]);
            let virtual_size = u32::from_le_bytes([
                section_header[8], section_header[9],
                section_header[10], section_header[11],
            ]);
            let pointer_to_raw_data = u32::from_le_bytes([
                section_header[20], section_header[21],
                section_header[22], section_header[23],
            ]);
            let size_of_raw_data = u32::from_le_bytes([
                section_header[16], section_header[17],
                section_header[18], section_header[19],
            ]);

            sections.push(SectionInfo {
                virtual_address,
                virtual_size,
                pointer_to_raw_data,
                size_of_raw_data,
            });

            trace!("Section {}: VA=0x{:08X}, Size=0x{:08X}, Raw=0x{:08X}",
                   i, virtual_address, virtual_size, pointer_to_raw_data);
        }

        // 将 RVA 转换为文件偏移
        let debug_file_offset = rva_to_file_offset(debug_rva, &sections);
        if debug_file_offset == 0 {
            trace!("Failed to convert debug directory RVA to file offset");
            return Ok(None);
        }

        // 读取调试目录条目
        if file.seek(SeekFrom::Start(debug_file_offset as u64)).is_err() {
            return Ok(None);
        }

        // 计算调试目录条目数量
        let debug_entry_count = debug_size / 28; // DEBUG_DIRECTORY 结构大小为 28 bytes
        trace!("Debug directory entries: {}", debug_entry_count);

        for i in 0..debug_entry_count {
            let mut debug_entry = [0u8; 28];
            if file.read_exact(&mut debug_entry).is_err() {
                break;
            }

            let characteristics = u32::from_le_bytes([debug_entry[0], debug_entry[1], debug_entry[2], debug_entry[3]]);
            let time_date_stamp = u32::from_le_bytes([debug_entry[4], debug_entry[5], debug_entry[6], debug_entry[7]]);
            let major_version = u16::from_le_bytes([debug_entry[8], debug_entry[9]]);
            let minor_version = u16::from_le_bytes([debug_entry[10], debug_entry[11]]);
            let debug_type = u32::from_le_bytes([debug_entry[12], debug_entry[13], debug_entry[14], debug_entry[15]]);
            let size_of_data = u32::from_le_bytes([debug_entry[16], debug_entry[17], debug_entry[18], debug_entry[19]]);
            let address_of_raw_data = u32::from_le_bytes([debug_entry[20], debug_entry[21], debug_entry[22], debug_entry[23]]);
            let pointer_to_raw_data = u32::from_le_bytes([debug_entry[24], debug_entry[25], debug_entry[26], debug_entry[27]]);

            trace!("Debug entry {}: Type={}, Size={}", i, debug_type, size_of_data);

            // 2 = IMAGE_DEBUG_TYPE_CODEVIEW
            if debug_type == 2 && size_of_data > 0 && pointer_to_raw_data > 0 {
                // 读取 CodeView 信息
                if let Some(pdb_info) = self.read_codeview_info(&mut file, pointer_to_raw_data, size_of_data) {
                    trace!("Found CodeView info: PDB path = {}", pdb_info.pdb_path);
                    return Ok(Some(pdb_info));
                }
            }
        }

        trace!("No CodeView debug info found in PE file");
        Ok(None)
    }

    /// 读取 CodeView 信息
    ///
    /// 支持 NB10 和 RSDS 格式
    fn read_codeview_info(&self, file: &mut fs::File, offset: u32, size: u32) -> Option<EmbeddedPdbInfo> {
        use std::io::{Read, Seek, SeekFrom};

        if file.seek(SeekFrom::Start(offset as u64)).is_err() {
            return None;
        }

        // 读取 CodeView 签名（4 bytes）
        let mut signature = [0u8; 4];
        if file.read_exact(&mut signature).is_err() {
            return None;
        }

        trace!("CodeView signature: {:?}", String::from_utf8_lossy(&signature));

        // 重置位置
        if file.seek(SeekFrom::Start(offset as u64)).is_err() {
            return None;
        }

        // RSDS 格式（较新的 PDB 7.0 格式）
        if &signature == b"RSDS" {
            // RSDS 格式结构：
            // 4 bytes: "RSDS" 签名
            // 16 bytes: GUID
            // 4 bytes: Age
            // n bytes: PDB 路径（以 null 结尾的 ASCII 字符串）

            let mut rsds_data = vec![0u8; size as usize];
            if file.read_exact(&mut rsds_data).is_err() {
                return None;
            }

            // 提取 GUID（16 bytes）
            let guid_bytes = &rsds_data[4..20];
            let guid = format!(
                "{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
                guid_bytes[0], guid_bytes[1], guid_bytes[2], guid_bytes[3],
                guid_bytes[4], guid_bytes[5], guid_bytes[6], guid_bytes[7],
                guid_bytes[8], guid_bytes[9], guid_bytes[10], guid_bytes[11],
                guid_bytes[12], guid_bytes[13], guid_bytes[14], guid_bytes[15]
            );

            // 提取 Age（4 bytes，little-endian）
            let age = u32::from_le_bytes([
                rsds_data[20], rsds_data[21], rsds_data[22], rsds_data[23]
            ]);

            // 提取 PDB 路径（从第 24 byte 开始，以 null 结尾）
            let pdb_path_start = 24;
            let pdb_path_bytes = &rsds_data[pdb_path_start..];
            let pdb_path = match pdb_path_bytes.iter().position(|&b| b == 0) {
                Some(null_pos) => String::from_utf8_lossy(&pdb_path_bytes[..null_pos]).to_string(),
                None => String::from_utf8_lossy(pdb_path_bytes).to_string(),
            };

            let raw = format!("{}/{}", guid, age);
            trace!("RSDS format: GUID={}, Age={}, PDB={}", guid, age, pdb_path);

            return Some(EmbeddedPdbInfo {
                pdb_path,
                signature: Some(PdbSignature {
                    guid,
                    age,
                    raw,
                }),
            });
        }

        // NB10 格式（较旧的 PDB 2.0 格式）
        if &signature == b"NB10" {
            // NB10 格式结构：
            // 4 bytes: "NB10" 签名
            // 4 bytes: Offset（未使用）
            // 4 bytes: Signature（时间戳）
            // 4 bytes: Age
            // n bytes: PDB 路径（以 null 结尾的 ASCII 字符串）

            let mut nb10_data = vec![0u8; size as usize];
            if file.read_exact(&mut nb10_data).is_err() {
                return None;
            }

            // 提取 Signature（时间戳）
            let signature = u32::from_le_bytes([
                nb10_data[4], nb10_data[5], nb10_data[6], nb10_data[7]
            ]);

            // 提取 Age
            let age = u32::from_le_bytes([
                nb10_data[8], nb10_data[9], nb10_data[10], nb10_data[11]
            ]);

            // 提取 PDB 路径
            let pdb_path_start = 12;
            let pdb_path_bytes = &nb10_data[pdb_path_start..];
            let pdb_path = match pdb_path_bytes.iter().position(|&b| b == 0) {
                Some(null_pos) => String::from_utf8_lossy(&pdb_path_bytes[..null_pos]).to_string(),
                None => String::from_utf8_lossy(pdb_path_bytes).to_string(),
            };

            let raw = format!("{:08X}{}", signature, age);

            trace!("NB10 format: Signature={:08X}, Age={}, PDB={}", signature, age, pdb_path);

            return Some(EmbeddedPdbInfo {
                pdb_path,
                signature: Some(PdbSignature {
                    guid: format!("{:08X}", signature),
                    age,
                    raw,
                }),
            });
        }

        trace!("Unknown CodeView signature: {:?}", String::from_utf8_lossy(&signature));
        None
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

    #[test]
    fn test_extract_pdb_info() {
        // 测试从 Debug 版本的 cpu_intensive.exe 中提取 PDB 信息
        let test_exe = PathBuf::from("target/debug/examples/cpu_intensive.exe");
        
        if !test_exe.exists() {
            println!("Test executable not found: {}", test_exe.display());
            return;
        }

        let locator = PdbLocator::new();
        let result = locator.extract_pdb_info(&test_exe);

        assert!(result.is_ok(), "extract_pdb_info should succeed");
        
        let pdb_info = result.unwrap();
        assert!(pdb_info.is_some(), "Should find PDB info in debug executable");
        
        let info = pdb_info.unwrap();
        println!("Found PDB path: {}", info.pdb_path);
        
        // 验证 PDB 路径不为空
        assert!(!info.pdb_path.is_empty(), "PDB path should not be empty");
        
        // 验证签名存在
        assert!(info.signature.is_some(), "Should have PDB signature");
        
        let sig = info.signature.unwrap();
        println!("PDB GUID: {}, Age: {}", sig.guid, sig.age);
        
        // 验证 GUID 不为空
        assert!(!sig.guid.is_empty(), "GUID should not be empty");
        
        // 验证 age 为正数
        assert!(sig.age > 0, "Age should be positive");
    }

    #[test]
    fn test_rva_to_file_offset() {
        // 测试 RVA 转换函数
        let sections = vec![
            SectionInfo {
                virtual_address: 0x1000,
                virtual_size: 0x1000,
                pointer_to_raw_data: 0x400,
                size_of_raw_data: 0x1000,
            },
        ];

        // RVA 在节范围内
        assert_eq!(rva_to_file_offset(0x1000, &sections), 0x400);
        assert_eq!(rva_to_file_offset(0x1500, &sections), 0x900);
        
        // RVA 不在节范围内（返回原始 RVA）
        assert_eq!(rva_to_file_offset(0x500, &sections), 0x500);
    }
}
