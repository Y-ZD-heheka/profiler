//! 符号管理器
//!
//! 管理多个进程的符号解析器，自动处理模块加载/卸载事件。
//! 提供统一的符号解析接口，处理跨进程的符号管理。

use crate::error::{Result, SymbolError};
use crate::symbols::{DbgHelpResolver, SymbolLoadCallback};
use crate::types::{Address, ModuleInfo, ProcessId, SymbolInfo};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use tracing::{debug, error, info, trace, warn};

/// 符号管理器
///
/// 管理多个进程的符号解析器实例，自动处理模块生命周期事件。
/// 提供统一的符号解析接口，适用于多进程性能分析场景。
///
/// # 线程安全
///
/// 所有方法都是线程安全的，可以在多线程环境中使用。
/// 内部使用 RwLock 保护解析器映射表。
///
/// # 使用示例
///
/// ```rust,no_run
/// use profiler::symbols::SymbolManager;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let mut manager = SymbolManager::new();
///
/// // 配置符号路径
/// manager.add_symbol_path("C:\\Symbols");
///
/// // 创建解析器
/// let resolver = manager.create_resolver(1234)?;
///
/// // 处理模块加载事件
/// manager.on_module_load(1234, &module_info)?;
///
/// // 解析采样地址
/// if let Some(symbol) = manager.resolve_sample(1234, 0x7ff123456789)? {
///     println!("Function: {}", symbol.name);
/// }
///
/// // 关闭时清理
/// manager.shutdown()?;
/// # Ok(())
/// # }
/// ```
pub struct SymbolManager {
    /// 进程 ID 到解析器的映射
    resolvers: RwLock<HashMap<ProcessId, Arc<Mutex<DbgHelpResolver>>>>,
    /// 符号搜索路径列表
    symbol_paths: Mutex<Vec<PathBuf>>,
    /// 是否自动加载模块符号
    auto_load_modules: bool,
    /// 全局符号缓存开关
    enable_caching: bool,
    /// 加载进度回调
    progress_callback: Option<Box<dyn SymbolLoadCallback>>,
}

/// 模块加载事件
#[derive(Debug, Clone)]
pub struct ModuleLoadEvent {
    /// 进程 ID
    pub process_id: ProcessId,
    /// 模块信息
    pub module: ModuleInfo,
    /// 模块文件路径
    pub module_path: PathBuf,
    /// 模块基地址
    pub base_address: Address,
    /// 模块大小
    pub size: u32,
}

/// 模块卸载事件
#[derive(Debug, Clone)]
pub struct ModuleUnloadEvent {
    /// 进程 ID
    pub process_id: ProcessId,
    /// 模块基地址
    pub base_address: Address,
    /// 模块名称
    pub module_name: String,
}

impl SymbolManager {
    /// 创建新的符号管理器
    ///
    /// 创建时使用默认配置：
    /// - 自动加载模块符号: 开启
    /// - 启用符号缓存: 开启
    /// - 符号路径: 空列表
    pub fn new() -> Self {
        info!("Creating SymbolManager");

        Self {
            resolvers: RwLock::new(HashMap::new()),
            symbol_paths: Mutex::new(Vec::new()),
            auto_load_modules: true,
            enable_caching: true,
            progress_callback: None,
        }
    }

    /// 创建符号管理器（带配置）
    ///
    /// # 参数
    /// - `symbol_paths`: 初始符号搜索路径
    /// - `auto_load`: 是否自动加载模块符号
    /// - `enable_cache`: 是否启用符号缓存
    pub fn with_config(
        symbol_paths: Vec<PathBuf>,
        auto_load: bool,
        enable_cache: bool,
    ) -> Self {
        info!(
            "Creating SymbolManager with {} paths, auto_load={}, cache={}",
            symbol_paths.len(),
            auto_load,
            enable_cache
        );

        Self {
            resolvers: RwLock::new(HashMap::new()),
            symbol_paths: Mutex::new(symbol_paths),
            auto_load_modules: auto_load,
            enable_caching: enable_cache,
            progress_callback: None,
        }
    }

    /// 创建符号解析器
    ///
    /// 为指定进程创建符号解析器。如果解析器已存在，返回现有实例。
    ///
    /// # 参数
    /// - `process_id`: 目标进程 ID
    ///
    /// # 返回
    /// 解析器的 Arc<Mutex<>> 包装，可以在多线程间共享
    pub fn create_resolver(&self, process_id: ProcessId) -> Result<Arc<Mutex<DbgHelpResolver>>> {
        // 首先尝试读取（不需要写锁）
        {
            let resolvers = self.resolvers.read().map_err(|_| {
                SymbolError::new("Failed to acquire read lock on resolvers")
            })?;

            if let Some(resolver) = resolvers.get(&process_id) {
                trace!("Returning existing resolver for process {}", process_id);
                return Ok(Arc::clone(resolver));
            }
        }

        // 需要创建新解析器
        let mut resolvers = self.resolvers.write().map_err(|_| {
            SymbolError::new("Failed to acquire write lock on resolvers")
        })?;

        // 双重检查
        if let Some(resolver) = resolvers.get(&process_id) {
            return Ok(Arc::clone(resolver));
        }

        info!("Creating new symbol resolver for process {}", process_id);

        // 创建解析器
        let mut resolver = DbgHelpResolver::new(process_id)?;

        // 初始化
        let paths = self.symbol_paths.lock().map_err(|_| {
            SymbolError::new("Failed to lock symbol paths")
        })?;
        resolver.initialize(&paths)?;

        let resolver = Arc::new(Mutex::new(resolver));
        resolvers.insert(process_id, Arc::clone(&resolver));

        info!("Symbol resolver created for process {}", process_id);
        Ok(resolver)
    }

    /// 处理模块加载事件
    ///
    /// 当检测到模块加载时调用，自动加载模块符号。
    ///
    /// # 参数
    /// - `process_id`: 进程 ID
    /// - `module`: 模块信息
    pub fn on_module_load(&self, process_id: ProcessId, module: &ModuleInfo) -> Result<()> {
        if !self.auto_load_modules {
            debug!("Auto-load disabled, skipping module load for {}", module.name);
            return Ok(());
        }

        trace!("Processing module load for {} in process {}", module.name, process_id);

        // 获取或创建解析器
        let resolver = self.create_resolver(process_id)?;
        let mut resolver = resolver.lock().map_err(|_| {
            SymbolError::new("Failed to lock resolver")
        })?;

        // 获取模块路径 - 优先使用完整路径
        let module_path = module
            .path
            .as_ref()
            .filter(|p| !p.is_empty() && *p != "<unknown>")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                warn!("Module path not available for {}, using name as fallback", module.name);
                PathBuf::from(&module.name)
            });

        // 验证路径是否存在
        let path_exists = module_path.exists();
        if !path_exists && module_path != PathBuf::from(&module.name) {
            warn!("Module path does not exist: {}", module_path.display());
        }

        info!("[MODULE_LOAD] Processing module load: {} at 0x{:016X} (size: {}, path: {}, exists: {})",
            module.name, module.base_address, module.size, module_path.display(), path_exists);

        // 诊断：检查模块路径是否有效
        if !path_exists {
            error!("[MODULE_LOAD] Module file does not exist: {}", module_path.display());
            error!("[MODULE_LOAD] Symbol resolution will fail for this module!");
        }

        // 诊断：检查 PDB 文件是否存在（多种位置）
        let pdb_path = module_path.with_extension("pdb");
        let pdb_in_exe_dir = module_path.parent()
            .map(|p| p.join(format!("{}.pdb", module.name)))
            .map(|p| p.exists())
            .unwrap_or(false);
        
        // 检查更多可能的 PDB 位置
        let mut pdb_locations = vec![pdb_path.clone()];
        if let Some(parent) = module_path.parent() {
            pdb_locations.push(parent.join(format!("{}.pdb", module.name)));
            // 也检查 target/debug 目录
            if let Some(grandparent) = parent.parent() {
                pdb_locations.push(grandparent.join("debug").join(format!("{}.pdb", module.name)));
            }
        }
        
        let pdb_exists = pdb_locations.iter().any(|p| p.exists());
        let existing_pdb_path = pdb_locations.iter().find(|p| p.exists());
        
        debug!(
            "PDB file check for {}: exists={}, locations_checked={}",
            module.name, pdb_exists, pdb_locations.len()
        );
        
        if let Some(p) = existing_pdb_path {
            debug!("Found PDB at: {}", p.display());
        }

        if !path_exists {
            warn!(
                "Module file does not exist at '{}'. Symbol resolution will likely fail. Ensure the executable/DLL path is correct and accessible.",
                module_path.display()
            );
        } else if !pdb_exists && !pdb_in_exe_dir {
            warn!(
                "PDB file not found for module '{}'. Looked at: {:?} and in executable directory. Symbol resolution will return addresses instead of function names. Build with debug symbols enabled (e.g., 'cargo build') to generate PDB files.",
                module.name, pdb_path
            );
        }
        
        // 尝试从模块文件中提取 PDB 信息（CodeView）
        if path_exists {
            use crate::symbols::PdbLocator;
            let locator = PdbLocator::new();
            match locator.extract_pdb_info(&module_path) {
                Ok(Some(pdb_info)) => {
                    info!("Extracted PDB info from module {}: path={}, signature={:?}",
                          module.name, pdb_info.pdb_path, pdb_info.signature);
                }
                Ok(None) => {
                    debug!("No embedded PDB info found in module {}", module.name);
                }
                Err(e) => {
                    trace!("Failed to extract PDB info from {}: {}", module.name, e);
                }
            }
        }

        // 加载模块符号
        match resolver.load_module(&module_path, module.base_address, module.size as u32) {
            Ok(_) => {
                debug!("Successfully loaded symbols for module: {}", module.name);
            }
            Err(e) => {
                error!("Failed to load symbols for module {} at 0x{:016X}: {}",
                    module.name, module.base_address, e);
                // 尝试使用仅文件名再次加载
                let name_only = PathBuf::from(&module.name);
                if name_only != module_path {
                    debug!("Retrying with module name only: {}", module.name);
                    if let Err(e2) = resolver.load_module(&name_only, module.base_address, module.size as u32) {
                        trace!("Retry also failed: {}", e2);
                    }
                }
            }
        }

        // 通知回调
        if let Some(callback) = &self.progress_callback {
            callback.on_complete(&module.name, true);
        }

        Ok(())
    }

    /// 处理模块卸载事件
    ///
    /// 当检测到模块卸载时调用，清理相关符号。
    ///
    /// # 参数
    /// - `process_id`: 进程 ID
    /// - `base_address`: 模块基地址
    pub fn on_module_unload(&self, process_id: ProcessId, base_address: Address) -> Result<()> {
        trace!("Processing module unload at 0x{:016X} in process {}", base_address, process_id);

        // 获取解析器
        let resolver = {
            let resolvers = self.resolvers.read().map_err(|_| {
                SymbolError::new("Failed to acquire read lock on resolvers")
            })?;

            match resolvers.get(&process_id) {
                Some(r) => Arc::clone(r),
                None => {
                    warn!("No resolver found for process {}", process_id);
                    return Ok(());
                }
            }
        };

        let mut resolver = resolver.lock().map_err(|_| {
            SymbolError::new("Failed to lock resolver")
        })?;

        resolver.unload_module(base_address)?;

        Ok(())
    }

    /// 解析采样事件
    ///
    /// 解析采样事件中的指令地址为符号信息。
    ///
    /// # 参数
    /// - `process_id`: 进程 ID
    /// - `instruction_pointer`: 指令指针地址
    ///
    /// # 返回
    /// 解析成功的符号信息，如果进程无解析器则返回 None
    pub fn resolve_sample(
        &self,
        process_id: ProcessId,
        instruction_pointer: Address,
    ) -> Result<Option<SymbolInfo>> {
        info!("[SAMPLE_RESOLVE] Resolving sample at 0x{:016X} for process {}", instruction_pointer, process_id);

        // 获取解析器
        let resolver = {
            let resolvers = self.resolvers.read().map_err(|_| {
                SymbolError::new("Failed to acquire read lock on resolvers")
            })?;

            match resolvers.get(&process_id) {
                Some(r) => {
                    info!("[SAMPLE_RESOLVE] Found existing resolver for process {}", process_id);
                    Arc::clone(r)
                }
                None => {
                    // 自动创建解析器
                    warn!("[SAMPLE_RESOLVE] No resolver found for process {}, creating new one", process_id);
                    drop(resolvers);
                    self.create_resolver(process_id)?
                }
            }
        };

        let resolver = resolver.lock().map_err(|_| {
            SymbolError::new("Failed to lock resolver")
        })?;

        // 检查该进程已加载的模块数
        match resolver.get_loaded_modules() {
            Ok(modules) => {
                info!("[SAMPLE_RESOLVE] Process {} has {} loaded modules", process_id, modules.len());
                if modules.is_empty() {
                    warn!("[SAMPLE_RESOLVE] No modules loaded for process {}! Symbol resolution will fail.", process_id);
                }
            }
            Err(e) => {
                warn!("[SAMPLE_RESOLVE] Could not get loaded modules for process {}: {}", process_id, e);
            }
        }

        match resolver.resolve_address(instruction_pointer) {
            Ok(symbol) => {
                // 检查解析结果是否是地址格式（表示解析失败）
                if symbol.name.starts_with("0x") {
                    warn!("[SAMPLE_RESOLVE] Address 0x{:016X} resolved to '{}' (raw address - symbol not found)", 
                        instruction_pointer, symbol.name);
                } else {
                    info!("[SAMPLE_RESOLVE] Address 0x{:016X} resolved to '{}'", instruction_pointer, symbol.name);
                }
                Ok(Some(symbol))
            }
            Err(e) => {
                error!("[SAMPLE_RESOLVE] Failed to resolve address 0x{:016X}: {}", instruction_pointer, e);
                Ok(None)
            }
        }
    }

    /// 解析堆栈采样
    ///
    /// # 参数
    /// - `process_id`: 进程 ID
    /// - `addresses`: 堆栈地址列表
    ///
    /// # 返回
    /// 解析后的堆栈帧列表
    pub fn resolve_stack(
        &self,
        process_id: ProcessId,
        addresses: &[Address],
    ) -> Result<Vec<crate::types::StackFrame>> {
        trace!("Resolving stack with {} frames for process {}", addresses.len(), process_id);

        let resolver = self.create_resolver(process_id)?;
        let resolver = resolver.lock().map_err(|_| {
            SymbolError::new("Failed to lock resolver")
        })?;

        resolver.resolve_stack(addresses)
    }

    /// 添加符号搜索路径
    ///
    /// # 参数
    /// - `path`: 要添加的符号路径
    pub fn add_symbol_path(&self, path: impl Into<PathBuf>) {
        let path = path.into();
        debug!("Adding symbol path: {}", path.display());

        if let Ok(mut paths) = self.symbol_paths.lock() {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }

    /// 设置符号搜索路径
    ///
    /// # 参数
    /// - `paths`: 新的符号路径列表（覆盖现有）
    pub fn set_symbol_paths(&self, paths: Vec<PathBuf>) {
        info!("Setting {} symbol paths", paths.len());

        if let Ok(mut current) = self.symbol_paths.lock() {
            *current = paths;
        }
    }

    /// 获取当前符号搜索路径
    pub fn get_symbol_paths(&self) -> Vec<PathBuf> {
        self.symbol_paths
            .lock()
            .map(|p| p.clone())
            .unwrap_or_default()
    }

    /// 设置进度回调
    ///
    /// # 参数
    /// - `callback`: 回调接口
    pub fn set_progress_callback(&mut self, callback: Box<dyn SymbolLoadCallback>) {
        self.progress_callback = Some(callback);
    }

    /// 清除进度回调
    pub fn clear_progress_callback(&mut self) {
        self.progress_callback = None;
    }

    /// 获取指定进程的解析器
    ///
    /// # 参数
    /// - `process_id`: 进程 ID
    ///
    /// # 返回
    /// 解析器引用，如果不存在返回 None
    pub fn get_resolver(&self, process_id: ProcessId) -> Option<Arc<Mutex<DbgHelpResolver>>> {
        let resolvers = self.resolvers.read().ok()?;
        resolvers.get(&process_id).map(Arc::clone)
    }

    /// 检查是否已存在解析器
    pub fn has_resolver(&self, process_id: ProcessId) -> bool {
        self.get_resolver(process_id).is_some()
    }

    /// 获取所有管理的进程 ID
    pub fn get_process_ids(&self) -> Vec<ProcessId> {
        let resolvers = self.resolvers.read().ok();
        match resolvers {
            Some(r) => r.keys().copied().collect(),
            None => Vec::new(),
        }
    }

    /// 获取已加载模块数量
    pub fn get_module_count(&self, process_id: ProcessId) -> usize {
        if let Some(resolver) = self.get_resolver(process_id) {
            if let Ok(resolver) = resolver.lock() {
                return resolver.get_loaded_modules().map(|m| m.len()).unwrap_or(0);
            }
        }
        0
    }

    /// 移除进程解析器
    ///
    /// # 参数
    /// - `process_id`: 要移除的进程 ID
    ///
    /// # 返回
    /// 如果成功移除返回 true
    pub fn remove_resolver(&self, process_id: ProcessId) -> bool {
        let mut resolvers = match self.resolvers.write() {
            Ok(r) => r,
            Err(_) => return false,
        };

        resolvers.remove(&process_id).is_some()
    }

    /// 清除指定进程的所有符号缓存
    pub fn clear_cache(&self, process_id: ProcessId) -> Result<()> {
        if let Some(resolver) = self.get_resolver(process_id) {
            let resolver = resolver.lock().map_err(|_| {
                SymbolError::new("Failed to lock resolver")
            })?;
            resolver.clear_cache()?;
        }
        Ok(())
    }

    /// 清除所有进程的符号缓存
    pub fn clear_all_caches(&self) -> Result<()> {
        let resolvers = self.resolvers.read().map_err(|_| {
            SymbolError::new("Failed to acquire read lock on resolvers")
        })?;

        for (_, resolver) in resolvers.iter() {
            if let Ok(r) = resolver.lock() {
                let _ = r.clear_cache();
            }
        }

        Ok(())
    }

    /// 关闭所有解析器
    ///
    /// 清理所有资源，关闭符号引擎。
    /// 应该在程序退出时调用。
    pub fn shutdown(&self) -> Result<()> {
        info!("Shutting down SymbolManager");

        let mut resolvers = self.resolvers.write().map_err(|_| {
            SymbolError::new("Failed to acquire write lock on resolvers")
        })?;

        let count = resolvers.len();
        resolvers.clear();

        info!("SymbolManager shutdown complete ({} resolvers cleared)", count);
        Ok(())
    }

    /// 设置自动加载模块
    pub fn set_auto_load(&mut self, auto_load: bool) {
        self.auto_load_modules = auto_load;
        debug!("Auto-load modules set to: {}", auto_load);
    }

    /// 获取自动加载设置
    pub fn is_auto_load_enabled(&self) -> bool {
        self.auto_load_modules
    }

    /// 设置缓存启用状态
    pub fn set_caching(&mut self, enable: bool) {
        self.enable_caching = enable;
        debug!("Symbol caching set to: {}", enable);
    }

    /// 获取缓存启用状态
    pub fn is_caching_enabled(&self) -> bool {
        self.enable_caching
    }

    /// 获取统计信息
    pub fn get_stats(&self) -> SymbolManagerStats {
        let resolver_count = self.resolvers.read().map(|r| r.len()).unwrap_or(0);

        SymbolManagerStats {
            resolver_count,
            symbol_paths: self.get_symbol_paths().len(),
            auto_load: self.auto_load_modules,
            caching: self.enable_caching,
        }
    }
}

impl Default for SymbolManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 符号管理器统计信息
#[derive(Debug, Clone)]
pub struct SymbolManagerStats {
    /// 解析器数量
    pub resolver_count: usize,
    /// 符号路径数量
    pub symbol_paths: usize,
    /// 自动加载状态
    pub auto_load: bool,
    /// 缓存状态
    pub caching: bool,
}

/// 便捷方法实现
trait ModuleInfoExt {
    fn with_path(self, path: String) -> Self;
}

impl ModuleInfoExt for ModuleInfo {
    fn with_path(mut self, path: String) -> Self {
        self.path = Some(path);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_manager_creation() {
        let manager = SymbolManager::new();
        assert_eq!(manager.get_process_ids().len(), 0);
        assert!(manager.is_auto_load_enabled());
        assert!(manager.is_caching_enabled());
    }

    #[test]
    fn test_symbol_manager_with_config() {
        let paths = vec![PathBuf::from("C:\\Symbols")];
        let manager = SymbolManager::with_config(paths, false, false);

        assert!(!manager.is_auto_load_enabled());
        assert!(!manager.is_caching_enabled());
        assert_eq!(manager.get_symbol_paths().len(), 1);
    }

    #[test]
    fn test_add_symbol_path() {
        let manager = SymbolManager::new();
        manager.add_symbol_path("C:\\Symbols");
        manager.add_symbol_path("D:\\MoreSymbols");

        let paths = manager.get_symbol_paths();
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn test_symbol_manager_stats() {
        let manager = SymbolManager::new();
        let stats = manager.get_stats();

        assert_eq!(stats.resolver_count, 0);
        assert_eq!(stats.symbol_paths, 0);
        assert!(stats.auto_load);
        assert!(stats.caching);
    }
}
