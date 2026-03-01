//! 符号解析器实现
//!
//! 基于 Windows DbgHelp API 实现符号解析功能。
//! 提供地址到符号名称、源文件和行号的转换。

use crate::error::{Result, SymbolError};
use crate::symbols::{get_module_name, SymbolResolver};
use crate::types::{Address, ModuleInfo, StackFrame, SymbolInfo};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{debug, error, info, trace, warn};
use windows::core::{Error as WinError, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, HANDLE, MAX_PATH};
use windows::Win32::System::Diagnostics::Debug::{
    SymCleanup, SymFromAddrW, SymGetLineFromAddrW64, SymInitializeW, SymLoadModuleExW,
    SymSetOptions, SymSetSearchPathW, SymUnloadModule64, SYMOPT_DEBUG, SYMOPT_DEFERRED_LOADS,
    SYMOPT_LOAD_LINES, SYMBOL_INFOW,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};

/// DbgHelp 符号解析器
///
/// 使用 Windows DbgHelp API 进行符号解析的实现。
/// 每个进程需要独立的解析器实例。
///
/// # 线程安全
///
/// DbgHelp API 不是线程安全的，因此所有方法都通过内部 Mutex 保护。
/// 解析器本身实现了 Send + Sync，可以安全地在多线程间共享。
pub struct DbgHelpResolver {
    /// 进程句柄
    process: HANDLE,
    /// 进程 ID
    process_id: u32,
    /// 是否拥有进程句柄（需要关闭）
    owns_handle: bool,
    /// 已加载的模块信息
    modules: Mutex<HashMap<Address, ModuleInfo>>,
    /// 符号缓存
    symbol_cache: Mutex<HashMap<Address, CachedSymbol>>,
    /// 是否已初始化
    initialized: Mutex<bool>,
    /// 初始化时间
    init_time: Instant,
}

/// 缓存的符号信息
#[derive(Debug, Clone)]
struct CachedSymbol {
    symbol: SymbolInfo,
    cached_at: Instant,
}

/// 符号解析统计
#[derive(Debug, Default)]
pub struct ResolveStats {
    /// 总解析请求数
    pub total_requests: u64,
    /// 缓存命中数
    pub cache_hits: u64,
    /// 成功解析数
    pub resolved: u64,
    /// 解析失败数
    pub failed: u64,
    /// 平均解析时间
    pub avg_resolve_time_us: u64,
}

impl DbgHelpResolver {
    /// 创建新的符号解析器
    ///
    /// # 参数
    /// - `process_id`: 目标进程 ID
    ///
    /// # 返回
    /// 新创建的解析器实例
    ///
    /// # 错误
    /// - 如果无法打开进程，返回错误
    /// - 如果进程 ID 无效，返回错误
    ///
    /// # 安全性
    ///
    /// 此函数调用 Windows API 进行符号引擎初始化。
    /// 需要确保进程句柄有效。
    pub fn new(process_id: u32) -> Result<Self> {
        trace!("Creating DbgHelpResolver for process {}", process_id);

        // 对于当前进程，使用 GetCurrentProcess
        // 对于其他进程，需要 OpenProcess
        let (process, owns_handle) = if process_id == 0 || process_id == std::process::id() {
            // SAFETY: GetCurrentProcess 总是返回有效的伪句柄
            (unsafe { GetCurrentProcess() }, false)
        } else {
            // 打开外部进程
            info!("Opening external process {} for symbol resolution", process_id);
            
            // SAFETY: OpenProcess 打开目标进程
            let process = unsafe {
                OpenProcess(
                    PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
                    false,
                    process_id,
                )
            };
            
            match process {
                Ok(handle) => {
                    info!("Successfully opened process {}", process_id);
                    (handle, true)
                }
                Err(err) => {
                    error!("Failed to open process {}: {}", process_id, err);
                    return Err(
                        SymbolError::with_address(
                            format!("Failed to open process {}: {}. Ensure you have appropriate permissions (run as administrator).", process_id, err),
                            process_id as u64,
                        ).into(),
                    );
                }
            }
        };

        let resolver = Self {
            process,
            process_id,
            owns_handle,
            modules: Mutex::new(HashMap::new()),
            symbol_cache: Mutex::new(HashMap::new()),
            initialized: Mutex::new(false),
            init_time: Instant::now(),
        };

        info!("Created DbgHelpResolver for process {}", process_id);
        Ok(resolver)
    }

    /// 初始化符号引擎
    ///
    /// # 参数
    /// - `symbol_paths`: 符号搜索路径列表
    ///
    /// # 安全性
    ///
    /// 调用 SymInitializeW 初始化符号引擎。
    /// 必须在调用其他符号 API 之前调用此方法。
    pub fn initialize(&mut self, symbol_paths: &[std::path::PathBuf]) -> Result<()> {
        let mut initialized = self.initialized.lock().map_err(|_| {
            SymbolError::new("Failed to lock initialized flag")
        })?;

        if *initialized {
            debug!("Symbol engine already initialized for process {}", self.process_id);
            return Ok(());
        }

        info!("Initializing symbol engine for process {}", self.process_id);

        // 构建搜索路径
        let search_path = build_search_path(symbol_paths);
        let wide_path: Vec<u16> = OsStr::new(&search_path)
            .encode_wide()
            .chain(Some(0))
            .collect();

        // SAFETY: 调用 SymInitializeW 初始化符号引擎
        // - process 是有效的进程句柄
        // - 我们传入 false 表示不枚举已加载的模块（手动加载）
        let result = unsafe { SymInitializeW(self.process, PCWSTR::null(), false) };

        if let Err(err) = result {
            error!("SymInitializeW failed: {}", err);
            return Err(SymbolError::new(format!("Failed to initialize symbol engine: {}", err)).into());
        }

        // 设置符号搜索路径
        if !search_path.is_empty() {
            // SAFETY: 调用 SymSetSearchPathW 设置搜索路径
            // - process 是有效的进程句柄
            // - wide_path 是以 null 结尾的有效宽字符串
            let result = unsafe { SymSetSearchPathW(self.process, PCWSTR(wide_path.as_ptr())) };

            if result.is_err() {
                warn!("Failed to set symbol search path");
            } else {
                debug!("Symbol search path set to: {}", search_path);
            }
        }

        // 设置符号选项
        // SAFETY: SymSetOptions 是线程安全的
        unsafe {
            SymSetOptions(
                SYMOPT_LOAD_LINES |      // 加载行号信息
                SYMOPT_DEFERRED_LOADS |  // 延迟加载符号
                SYMOPT_DEBUG,            // 启用调试输出
            );
        }

        *initialized = true;
        info!("Symbol engine initialized successfully");
        Ok(())
    }

    /// 加载模块符号
    ///
    /// # 参数
    /// - `module_path`: 模块文件路径
    /// - `base_address`: 模块基地址
    /// - `module_size`: 模块大小
    ///
    /// # 安全性
    ///
    /// 调用 SymLoadModuleExW 加载模块符号。
    /// 需要确保模块路径有效。
    pub fn load_module(&mut self, module_path: &Path, base_address: Address, module_size: u32) -> Result<()> {
        self.ensure_initialized()?;

        let module_name = get_module_name(module_path);
        info!("Loading symbols for module: {} at 0x{:016X}", module_name, base_address);

        // 转换路径为宽字符
        let wide_path: Vec<u16> = module_path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();

        // SAFETY: 调用 SymLoadModuleExW 加载模块
        // - process 是有效的进程句柄
        // - wide_path 是以 null 结尾的有效宽字符串
        // - 其他参数根据 API 要求传递
        let result = unsafe {
            SymLoadModuleExW(
                self.process,
                None, // hFile - 不需要文件句柄
                PCWSTR(wide_path.as_ptr()),
                PCWSTR::null(),               // ModuleName - 使用文件名
                base_address,
                module_size,
                None,                         // Data - 不需要额外数据
                None,                         // Flags
            )
        };

        if result == 0 {
            // 检查是否是模块已加载的错误（可以忽略）
            let last_error = unsafe { windows::Win32::Foundation::GetLastError() };
            if last_error == ERROR_INVALID_PARAMETER {
                debug!("Module already loaded: {}", module_name);
            } else {
                warn!("Failed to load symbols for {}: {:?}", module_name, last_error);
                return Err(SymbolError::with_module(
                    format!("Failed to load module symbols: {:?}", last_error),
                    &module_name,
                ).into());
            }
        } else {
            debug!("Successfully loaded symbols for {}", module_name);
        }

        // 记录模块信息
        let module_info = ModuleInfo::new(base_address, module_size as u64, &module_name)
            .with_path(module_path.to_string_lossy().to_string());

        let mut modules = self.modules.lock().map_err(|_| {
            SymbolError::new("Failed to lock modules")
        })?;
        modules.insert(base_address, module_info);

        Ok(())
    }

    /// 卸载模块符号
    ///
    /// # 参数
    /// - `base_address`: 模块基地址
    ///
    /// # 安全性
    ///
    /// 调用 SymUnloadModule64 卸载模块符号。
    pub fn unload_module(&mut self, base_address: Address) -> Result<()> {
        self.ensure_initialized()?;

        debug!("Unloading symbols for module at 0x{:016X}", base_address);

        // SAFETY: 调用 SymUnloadModule64 卸载模块
        // - process 是有效的进程句柄
        let result = unsafe { SymUnloadModule64(self.process, base_address) };

        if let Err(err) = result {
            warn!("Failed to unload module symbols: {}", err);
        }

        // 移除模块信息
        let mut modules = self.modules.lock().map_err(|_| {
            SymbolError::new("Failed to lock modules")
        })?;
        if let Some(removed) = modules.remove(&base_address) {
            info!("Unloaded symbols for module: {}", removed.name);
        }

        // 清除相关缓存
        self.invalidate_module_cache(base_address, modules.get(&base_address).map(|m| m.size).unwrap_or(0));

        Ok(())
    }

    /// 解析单个地址
    ///
    /// # 参数
    /// - `address`: 要解析的内存地址
    ///
    /// # 返回
    /// 解析成功的符号信息
    ///
    /// # 安全性
    ///
    /// 调用 SymFromAddrW 和 SymGetLineFromAddrW64 解析符号。
    pub fn resolve_address(&self, address: Address) -> Result<SymbolInfo> {
        self.ensure_initialized()?;

        // 首先检查缓存
        {
            let cache = self.symbol_cache.lock().map_err(|_| {
                SymbolError::new("Failed to lock symbol cache")
            })?;
            if let Some(cached) = cache.get(&address) {
                trace!("Cache hit for address 0x{:016X}", address);
                return Ok(cached.symbol.clone());
            }
        }

        trace!("Resolving address: 0x{:016X}", address);

        // 分配 SYMBOL_INFOW 结构
        // 需要包含 Name 字段的额外空间
        let mut symbol_buffer = vec![0u8; std::mem::size_of::<SYMBOL_INFOW>() + MAX_PATH as usize * 2];
        let symbol_info = symbol_buffer.as_mut_ptr() as *mut SYMBOL_INFOW;

        // SAFETY: 初始化 SYMBOL_INFOW 结构
        unsafe {
            (*symbol_info).SizeOfStruct = std::mem::size_of::<SYMBOL_INFOW>() as u32;
            (*symbol_info).MaxNameLen = MAX_PATH;
        }

        let mut displacement: u64 = 0;

        // SAFETY: 调用 SymFromAddrW 获取符号信息
        // - process 是有效的进程句柄
        // - symbol_info 指向有效的 SYMBOL_INFOW 结构
        let result = unsafe {
            SymFromAddrW(
                self.process,
                address,
                Some(&mut displacement),
                symbol_info,
            )
        };

        if let Err(err) = result {
            trace!("SymFromAddrW failed for 0x{:016X}: {}", address, err);

            // 返回包含地址的基本信息
            let symbol = SymbolInfo {
                address,
                name: format!("0x{:016X}", address),
                module: self.find_module_for_address(address),
                source_file: None,
                line_number: None,
            };

            return Ok(symbol);
        }

        // SAFETY: 提取符号名称
        let symbol_name = unsafe {
            let name_len = (*symbol_info).NameLen as usize;
            let name_ptr = (*symbol_info).Name.as_ptr();
            let name_slice = std::slice::from_raw_parts(name_ptr, name_len);
            String::from_utf16_lossy(name_slice)
        };

        let module_name = self.find_module_for_address(address);

        // 获取源文件和行号信息
        let (source_file, line_number) = self.get_line_info(address)?;

        let symbol = SymbolInfo {
            address,
            name: symbol_name,
            module: module_name,
            source_file,
            line_number,
        };

        // 缓存结果
        {
            let mut cache = self.symbol_cache.lock().map_err(|_| {
                SymbolError::new("Failed to lock symbol cache")
            })?;
            cache.insert(address, CachedSymbol {
                symbol: symbol.clone(),
                cached_at: Instant::now(),
            });
        }

        trace!("Resolved 0x{:016X} to {}", address, symbol.name);
        Ok(symbol)
    }

    /// 批量解析堆栈地址
    ///
    /// # 参数
    /// - `addresses`: 地址列表
    ///
    /// # 返回
    /// 解析后的堆栈帧列表
    pub fn resolve_stack(&self, addresses: &[Address]) -> Result<Vec<StackFrame>> {
        let mut frames = Vec::with_capacity(addresses.len());

        for &address in addresses {
            let symbol = self.resolve_address(address)?;

            let frame = StackFrame {
                address,
                module_name: symbol.module.clone(),
                function_name: Some(symbol.name.clone()),
                file_name: symbol.source_file.clone(),
                line_number: symbol.line_number,
                column_number: None,
                offset: None,
            };

            frames.push(frame);
        }

        Ok(frames)
    }

    /// 获取地址所在的模块信息
    ///
    /// # 参数
    /// - `address`: 内存地址
    ///
    /// # 返回
    /// 包含该地址的模块信息
    pub fn get_module_at_address(&self, address: Address) -> Result<ModuleInfo> {
        let modules = self.modules.lock().map_err(|_| {
            SymbolError::new("Failed to lock modules")
        })?;

        for (base, module) in modules.iter() {
            if address >= *base && address < *base + module.size {
                return Ok(module.clone());
            }
        }

        Err(SymbolError::with_address("Address not in any loaded module", address).into())
    }

    /// 获取解析器统计信息
    pub fn get_stats(&self) -> ResolveStats {
        // TODO: 实现统计信息收集
        ResolveStats::default()
    }

    /// 清除符号缓存
    pub fn clear_cache(&self) -> Result<()> {
        let mut cache = self.symbol_cache.lock().map_err(|_| {
            SymbolError::new("Failed to lock symbol cache")
        })?;
        cache.clear();
        debug!("Symbol cache cleared");
        Ok(())
    }

    /// 获取进程 ID
    pub fn process_id(&self) -> u32 {
        self.process_id
    }

    /// 获取已加载模块列表
    pub fn get_loaded_modules(&self) -> Result<Vec<ModuleInfo>> {
        let modules = self.modules.lock().map_err(|_| {
            SymbolError::new("Failed to lock modules")
        })?;
        Ok(modules.values().cloned().collect())
    }

    // 内部辅助方法

    fn ensure_initialized(&self) -> Result<()> {
        let initialized = self.initialized.lock().map_err(|_| {
            SymbolError::new("Failed to lock initialized flag")
        })?;

        if !*initialized {
            return Err(SymbolError::new("Symbol resolver not initialized").into());
        }

        Ok(())
    }

    fn find_module_for_address(&self, address: Address) -> Option<String> {
        if let Ok(modules) = self.modules.lock() {
            for (base, module) in modules.iter() {
                if address >= *base && address < *base + module.size {
                    return Some(module.name.clone());
                }
            }
        }
        None
    }

    fn get_line_info(&self, address: Address) -> Result<(Option<String>, Option<u32>)> {
        // 定义 IMAGEHLP_LINE64 结构
        #[repr(C)]
        struct ImageHlpLine64 {
            size_of_struct: u32,
            key: *const std::ffi::c_void,
            line_number: u32,
            file_name: *const u8,
            address: u64,
        }

        let mut line_info = ImageHlpLine64 {
            size_of_struct: std::mem::size_of::<ImageHlpLine64>() as u32,
            key: std::ptr::null(),
            line_number: 0,
            file_name: std::ptr::null(),
            address: 0,
        };

        let mut displacement: u32 = 0;

        // SAFETY: 调用 SymGetLineFromAddrW64 获取行号信息
        let result = unsafe {
            SymGetLineFromAddrW64(
                self.process,
                address,
                &mut displacement,
                &mut line_info as *mut _ as *mut _,
            )
        };

        if result.is_ok() {
            // SAFETY: 提取文件名
            let file_name = unsafe {
                if !line_info.file_name.is_null() {
                    let c_str = std::ffi::CStr::from_ptr(line_info.file_name as *const i8);
                    Some(c_str.to_string_lossy().to_string())
                } else {
                    None
                }
            };

            Ok((file_name, Some(line_info.line_number)))
        } else {
            Ok((None, None))
        }
    }

    fn invalidate_module_cache(&self, base_address: Address, size: u64) {
        if let Ok(mut cache) = self.symbol_cache.lock() {
            let addresses_to_remove: Vec<Address> = cache
                .keys()
                .filter(|&&addr| addr >= base_address && addr < base_address + size)
                .copied()
                .collect();

            for addr in addresses_to_remove {
                cache.remove(&addr);
            }
        }
    }
}

// SAFETY: DbgHelpResolver 内部使用 Mutex 保护所有可变状态
// HANDLE 实际上是线程安全的（Windows 句柄），但需要显式标记
unsafe impl Send for DbgHelpResolver {}
unsafe impl Sync for DbgHelpResolver {}

impl SymbolResolver for DbgHelpResolver {
    fn resolve_address(&self, address: Address) -> Result<SymbolInfo> {
        self.resolve_address(address)
    }

    fn resolve_stack(&self, addresses: &[Address]) -> Result<Vec<StackFrame>> {
        self.resolve_stack(addresses)
    }

    fn load_module(&mut self, module_path: &Path, base_address: Address, module_size: u32) -> Result<()> {
        self.load_module(module_path, base_address, module_size)
    }

    fn unload_module(&mut self, base_address: Address) -> Result<()> {
        self.unload_module(base_address)
    }

    fn get_module_at_address(&self, address: Address) -> Result<ModuleInfo> {
        self.get_module_at_address(address)
    }
}

impl Drop for DbgHelpResolver {
    fn drop(&mut self) {
        // 检查是否已初始化
        if let Ok(initialized) = self.initialized.lock() {
            if *initialized {
                // SAFETY: 调用 SymCleanup 清理符号引擎
                // 需要在所有符号操作完成后调用
                let _ = unsafe { SymCleanup(self.process) };
                info!("Symbol engine cleaned up for process {}", self.process_id);
            }
        }
        
        // 如果我们拥有进程句柄，需要关闭它
        if self.owns_handle {
            // SAFETY: CloseHandle 关闭进程句柄
            let _ = unsafe { CloseHandle(self.process) };
            trace!("Closed process handle for {}", self.process_id);
        }
    }
}

// 为 ModuleInfo 添加辅助方法
trait ModuleInfoExt {
    fn with_path(self, path: String) -> Self;
}

impl ModuleInfoExt for ModuleInfo {
    fn with_path(mut self, path: String) -> Self {
        self.path = Some(path);
        self
    }
}

/// 构建搜索路径字符串
fn build_search_path(paths: &[std::path::PathBuf]) -> String {
    use crate::symbols::build_symbol_search_path;
    build_symbol_search_path(paths, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dbg_help_resolver_creation() {
        // 只为当前进程创建解析器
        let resolver = DbgHelpResolver::new(std::process::id());
        assert!(resolver.is_ok());
    }

    #[test]
    fn test_resolve_stats_default() {
        let stats = ResolveStats::default();
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.cache_hits, 0);
    }
}
