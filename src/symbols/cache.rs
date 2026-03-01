//! 符号缓存模块
//!
//! 提供解析结果的内存缓存功能，减少重复的符号解析开销。
//! 使用 LRU 策略限制缓存大小，避免内存无限增长。

use crate::error::{Result, SymbolError};
use crate::types::{Address, ModuleInfo, SymbolInfo};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{debug, trace, warn};

/// 默认缓存条目最大数量
const DEFAULT_MAX_ENTRIES: usize = 100_000;

/// 默认缓存条目过期时间
const DEFAULT_ENTRY_TTL: Duration = Duration::from_secs(300); // 5 分钟

/// 符号缓存
///
/// 存储地址到符号信息的映射，使用 LRU 策略管理。
/// 支持按模块失效缓存，处理模块重载场景。
///
/// # 线程安全
///
/// 所有操作都通过 Mutex 保护，可以在多线程环境中安全使用。
///
/// # 使用示例
///
/// ```rust
/// use profiler::symbols::SymbolCache;
/// use profiler::types::{Address, SymbolInfo};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let mut cache = SymbolCache::new();
///
/// // 插入符号
/// let symbol = SymbolInfo {
///     address: 0x7ff123456789,
///     name: "MyFunction".to_string(),
///     module: Some("my.dll".to_string()),
///     source_file: Some("my.cpp".to_string()),
///     line_number: Some(42),
/// };
///
/// cache.insert(0x7ff123456789, symbol)?;
///
/// // 查询符号
/// if let Some(cached) = cache.get(0x7ff123456789) {
///     println!("Cached symbol: {}", cached.name);
/// }
///
/// // 使模块缓存失效
/// cache.invalidate_module(0x7ff10000000)?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct SymbolCache {
    /// 符号缓存映射
    cache: Mutex<HashMap<Address, CachedEntry>>,
    /// 模块缓存映射
    module_cache: Mutex<HashMap<Address, ModuleInfo>>,
    /// 最大条目数
    max_entries: usize,
    /// 条目过期时间
    entry_ttl: Duration,
    /// 命中次数统计
    hits: Mutex<u64>,
    /// 未命中次数统计
    misses: Mutex<u64>,
    /// 插入次数统计
    inserts: Mutex<u64>,
    /// 驱逐次数统计
    evictions: Mutex<u64>,
}

/// 缓存条目
#[derive(Debug, Clone)]
struct CachedEntry {
    /// 符号信息
    symbol: SymbolInfo,
    /// 插入时间
    inserted_at: Instant,
    /// 最后访问时间
    last_accessed: Instant,
    /// 访问次数
    access_count: u64,
}

impl CachedEntry {
    /// 检查条目是否过期
    fn is_expired(&self, ttl: Duration) -> bool {
        self.inserted_at.elapsed() > ttl
    }

    /// 创建新条目
    fn new(symbol: SymbolInfo) -> Self {
        let now = Instant::now();
        Self {
            symbol,
            inserted_at: now,
            last_accessed: now,
            access_count: 1,
        }
    }

    /// 记录访问
    fn record_access(&mut self) {
        self.last_accessed = Instant::now();
        self.access_count += 1;
    }
}

/// 缓存统计信息
#[derive(Debug, Clone, Copy)]
pub struct CacheStats {
    /// 当前条目数
    pub entry_count: usize,
    /// 当前模块数
    pub module_count: usize,
    /// 命中次数
    pub hits: u64,
    /// 未命中次数
    pub misses: u64,
    /// 插入次数
    pub inserts: u64,
    /// 驱逐次数
    pub evictions: u64,
    /// 命中率（0.0 - 1.0）
    pub hit_rate: f64,
}

impl SymbolCache {
    /// 创建新的符号缓存
    ///
    /// 使用默认配置：
    /// - 最大条目数: 100,000
    /// - 条目过期时间: 5 分钟
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_MAX_ENTRIES, DEFAULT_ENTRY_TTL)
    }

    /// 创建指定容量的符号缓存
    ///
    /// # 参数
    /// - `max_entries`: 最大缓存条目数
    /// - `entry_ttl`: 条目过期时间
    pub fn with_capacity(max_entries: usize, entry_ttl: Duration) -> Self {
        debug!(
            "Creating SymbolCache with max_entries={} and ttl={:?}",
            max_entries, entry_ttl
        );

        Self {
            cache: Mutex::new(HashMap::with_capacity(max_entries.min(1000))),
            module_cache: Mutex::new(HashMap::new()),
            max_entries,
            entry_ttl,
            hits: Mutex::new(0),
            misses: Mutex::new(0),
            inserts: Mutex::new(0),
            evictions: Mutex::new(0),
        }
    }

    /// 获取缓存的符号
    ///
    /// # 参数
    /// - `address`: 符号地址
    ///
    /// # 返回
    /// 缓存的符号信息，如果不存在或已过期返回 None
    pub fn get(&self, address: Address) -> Option<SymbolInfo> {
        let mut cache = match self.cache.lock() {
            Ok(c) => c,
            Err(_) => return None,
        };

        // 清理过期条目
        self.cleanup_expired(&mut cache);

        if let Some(entry) = cache.get_mut(&address) {
            if entry.is_expired(self.entry_ttl) {
                // 条目已过期，移除并返回 None
                trace!("Cache entry expired for address 0x{:016X}", address);
                cache.remove(&address);
                self.record_miss();
                return None;
            }

            // 更新访问统计
            entry.record_access();
            self.record_hit();
            trace!("Cache hit for address 0x{:016X}", address);
            Some(entry.symbol.clone())
        } else {
            self.record_miss();
            trace!("Cache miss for address 0x{:016X}", address);
            None
        }
    }

    /// 插入符号到缓存
    ///
    /// # 参数
    /// - `address`: 符号地址
    /// - `symbol`: 符号信息
    ///
    /// # 错误
    /// 如果无法获取锁，返回错误
    pub fn insert(&self, address: Address, symbol: SymbolInfo) -> Result<()> {
        let mut cache = self.cache.lock().map_err(|_| {
            SymbolError::new("Failed to lock symbol cache")
        })?;

        // 如果达到最大容量，执行 LRU 驱逐
        if cache.len() >= self.max_entries && !cache.contains_key(&address) {
            self.evict_lru(&mut cache);
        }

        let entry = CachedEntry::new(symbol);
        cache.insert(address, entry);

        self.record_insert();
        trace!("Inserted symbol at address 0x{:016X} into cache", address);

        Ok(())
    }

    /// 批量插入符号
    ///
    /// # 参数
    /// - `symbols`: (地址, 符号) 元组列表
    pub fn insert_batch(&self, symbols: &[(Address, SymbolInfo)]) -> Result<()> {
        let mut cache = self.cache.lock().map_err(|_| {
            SymbolError::new("Failed to lock symbol cache")
        })?;

        for (address, symbol) in symbols {
            // 检查是否需要驱逐
            if cache.len() >= self.max_entries && !cache.contains_key(address) {
                self.evict_lru(&mut cache);
            }

            let entry = CachedEntry::new(symbol.clone());
            cache.insert(*address, entry);
        }

        self.record_insert_batch(symbols.len() as u64);
        trace!("Inserted {} symbols into cache", symbols.len());

        Ok(())
    }

    /// 使模块缓存失效
    ///
    /// 移除指定模块地址范围内的所有缓存条目。
    /// 在模块卸载或重载时调用。
    ///
    /// # 参数
    /// - `base_address`: 模块基地址
    pub fn invalidate_module(&self, base_address: Address) -> Result<()> {
        // 获取模块大小
        let module_size = {
            let module_cache = self.module_cache.lock().map_err(|_| {
                SymbolError::new("Failed to lock module cache")
            })?;

            module_cache
                .get(&base_address)
                .map(|m| m.size)
                .unwrap_or(0)
        };

        self.invalidate_address_range(base_address, module_size)
    }

    /// 使地址范围缓存失效
    ///
    /// # 参数
    /// - `start_address`: 起始地址
    /// - `size`: 范围大小
    pub fn invalidate_address_range(&self, start_address: Address, size: u64) -> Result<()> {
        let mut cache = self.cache.lock().map_err(|_| {
            SymbolError::new("Failed to lock symbol cache")
        })?;

        let end_address = start_address.saturating_add(size);

        // 收集要移除的地址
        let addresses_to_remove: Vec<Address> = cache
            .keys()
            .filter(|&&addr| addr >= start_address && addr < end_address)
            .copied()
            .collect();

        let removed_count = addresses_to_remove.len();
        for addr in addresses_to_remove {
            cache.remove(&addr);
        }

        // 也从模块缓存中移除
        let mut module_cache = self.module_cache.lock().map_err(|_| {
            SymbolError::new("Failed to lock module cache")
        })?;
        module_cache.remove(&start_address);

        debug!(
            "Invalidated {} cache entries for range 0x{:016X} - 0x{:016X}",
            removed_count, start_address, end_address
        );

        Ok(())
    }

    /// 缓存模块信息
    ///
    /// # 参数
    /// - `module`: 模块信息
    pub fn cache_module(&self, module: ModuleInfo) -> Result<()> {
        let base_address = module.base_address;

        let mut module_cache = self.module_cache.lock().map_err(|_| {
            SymbolError::new("Failed to lock module cache")
        })?;

        module_cache.insert(base_address, module);
        trace!("Cached module at address 0x{:016X}", base_address);

        Ok(())
    }

    /// 获取缓存的模块信息
    ///
    /// # 参数
    /// - `base_address`: 模块基地址
    ///
    /// # 返回
    /// 模块信息，如果不存在返回 None
    pub fn get_module(&self, base_address: Address) -> Option<ModuleInfo> {
        let module_cache = self.module_cache.lock().ok()?;
        module_cache.get(&base_address).cloned()
    }

    /// 根据地址查找模块
    ///
    /// # 参数
    /// - `address`: 内存地址
    ///
    /// # 返回
    /// 包含该地址的模块信息
    pub fn find_module_for_address(&self, address: Address) -> Option<ModuleInfo> {
        let module_cache = self.module_cache.lock().ok()?;

        for (base, module) in module_cache.iter() {
            if address >= *base && address < *base + module.size {
                return Some(module.clone());
            }
        }

        None
    }

    /// 获取模块缓存大小
    pub fn module_cache_size(&self) -> usize {
        self.module_cache
            .lock()
            .map(|m| m.len())
            .unwrap_or(0)
    }

    /// 清除所有缓存
    pub fn clear(&self) -> Result<()> {
        let mut cache = self.cache.lock().map_err(|_| {
            SymbolError::new("Failed to lock symbol cache")
        })?;
        cache.clear();

        let mut module_cache = self.module_cache.lock().map_err(|_| {
            SymbolError::new("Failed to lock module cache")
        })?;
        module_cache.clear();

        // 重置统计
        if let Ok(mut hits) = self.hits.lock() {
            *hits = 0;
        }
        if let Ok(mut misses) = self.misses.lock() {
            *misses = 0;
        }
        if let Ok(mut inserts) = self.inserts.lock() {
            *inserts = 0;
        }
        if let Ok(mut evictions) = self.evictions.lock() {
            *evictions = 0;
        }

        debug!("Symbol cache cleared");
        Ok(())
    }

    /// 获取缓存大小（条目数）
    pub fn size(&self) -> usize {
        self.cache
            .lock()
            .map(|c| c.len())
            .unwrap_or(0)
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.size() == 0
    }

    /// 获取统计信息
    pub fn stats(&self) -> CacheStats {
        let entry_count = self.size();
        let module_count = self.module_cache_size();

        let hits = self.hits.lock().map(|h| *h).unwrap_or(0);
        let misses = self.misses.lock().map(|m| *m).unwrap_or(0);
        let inserts = self.inserts.lock().map(|i| *i).unwrap_or(0);
        let evictions = self.evictions.lock().map(|e| *e).unwrap_or(0);

        let total_requests = hits + misses;
        let hit_rate = if total_requests > 0 {
            hits as f64 / total_requests as f64
        } else {
            0.0
        };

        CacheStats {
            entry_count,
            module_count,
            hits,
            misses,
            inserts,
            evictions,
            hit_rate,
        }
    }

    /// 设置最大条目数
    ///
    /// # 参数
    /// - `max_entries`: 新的最大条目数
    pub fn set_max_entries(&mut self, max_entries: usize) {
        self.max_entries = max_entries;
        debug!("Symbol cache max_entries set to {}", max_entries);
    }

    /// 设置条目过期时间
    ///
    /// # 参数
    /// - `ttl`: 新的过期时间
    pub fn set_ttl(&mut self, ttl: Duration) {
        self.entry_ttl = ttl;
        debug!("Symbol cache TTL set to {:?}", ttl);
    }

    /// 获取最大条目数
    pub fn max_entries(&self) -> usize {
        self.max_entries
    }

    /// 获取条目过期时间
    pub fn ttl(&self) -> Duration {
        self.entry_ttl
    }

    // 内部辅助方法

    fn record_hit(&self) {
        if let Ok(mut hits) = self.hits.lock() {
            *hits += 1;
        }
    }

    fn record_miss(&self) {
        if let Ok(mut misses) = self.misses.lock() {
            *misses += 1;
        }
    }

    fn record_insert(&self) {
        if let Ok(mut inserts) = self.inserts.lock() {
            *inserts += 1;
        }
    }

    fn record_insert_batch(&self, count: u64) {
        if let Ok(mut inserts) = self.inserts.lock() {
            *inserts += count;
        }
    }

    fn record_eviction(&self) {
        if let Ok(mut evictions) = self.evictions.lock() {
            *evictions += 1;
        }
    }

    fn cleanup_expired(&self, cache: &mut HashMap<Address, CachedEntry>) {
        let expired_addresses: Vec<Address> = cache
            .iter()
            .filter(|(_, entry)| entry.is_expired(self.entry_ttl))
            .map(|(addr, _)| *addr)
            .collect();

        for addr in expired_addresses {
            cache.remove(&addr);
        }
    }

    fn evict_lru(&self, cache: &mut HashMap<Address, CachedEntry>) {
        if cache.is_empty() {
            return;
        }

        // 找到最少访问的条目
        let lru_address = cache
            .iter()
            .min_by_key(|(_, entry)| (entry.access_count, entry.last_accessed))
            .map(|(addr, _)| *addr);

        if let Some(addr) = lru_address {
            cache.remove(&addr);
            self.record_eviction();
            trace!("Evicted LRU cache entry at 0x{:016X}", addr);
        }
    }
}

impl Default for SymbolCache {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for CacheStats {
    fn default() -> Self {
        Self {
            entry_count: 0,
            module_count: 0,
            hits: 0,
            misses: 0,
            inserts: 0,
            evictions: 0,
            hit_rate: 0.0,
        }
    }
}

/// 线程安全的符号缓存包装
///
/// 提供 Arc<SymbolCache> 的便捷访问方式。
#[derive(Debug, Clone)]
pub struct SharedSymbolCache(Arc<SymbolCache>);

impl SharedSymbolCache {
    /// 创建新的共享缓存
    pub fn new() -> Self {
        Self(Arc::new(SymbolCache::new()))
    }

    /// 创建指定容量的共享缓存
    pub fn with_capacity(max_entries: usize, entry_ttl: Duration) -> Self {
        Self(Arc::new(SymbolCache::with_capacity(max_entries, entry_ttl)))
    }

    /// 获取内部缓存引用
    pub fn cache(&self) -> &SymbolCache {
        &self.0
    }

    /// 转换为 Arc
    pub fn into_inner(self) -> Arc<SymbolCache> {
        self.0
    }
}

impl Default for SharedSymbolCache {
    fn default() -> Self {
        Self::new()
    }
}

impl std::ops::Deref for SharedSymbolCache {
    type Target = SymbolCache;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_symbol(address: Address, name: &str) -> SymbolInfo {
        SymbolInfo {
            address,
            name: name.to_string(),
            module: Some("test.dll".to_string()),
            source_file: Some("test.cpp".to_string()),
            line_number: Some(42),
        }
    }

    #[test]
    fn test_symbol_cache_creation() {
        let cache = SymbolCache::new();
        assert!(cache.is_empty());
        assert_eq!(cache.size(), 0);
        assert_eq!(cache.max_entries(), DEFAULT_MAX_ENTRIES);
    }

    #[test]
    fn test_symbol_cache_with_capacity() {
        let cache = SymbolCache::with_capacity(1000, Duration::from_secs(60));
        assert_eq!(cache.max_entries(), 1000);
        assert_eq!(cache.ttl(), Duration::from_secs(60));
    }

    #[test]
    fn test_cache_insert_and_get() {
        let cache = SymbolCache::new();
        let symbol = create_test_symbol(0x7ff123456789, "TestFunction");

        cache.insert(0x7ff123456789, symbol.clone()).unwrap();

        let retrieved = cache.get(0x7ff123456789);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "TestFunction");
    }

    #[test]
    fn test_cache_miss() {
        let cache = SymbolCache::new();

        let retrieved = cache.get(0x7ff00000000);
        assert!(retrieved.is_none());

        let stats = cache.stats();
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hits, 0);
    }

    #[test]
    fn test_cache_hit_rate() {
        let cache = SymbolCache::new();
        let symbol = create_test_symbol(0x7ff123456789, "TestFunction");

        cache.insert(0x7ff123456789, symbol).unwrap();

        // 命中
        cache.get(0x7ff123456789);
        cache.get(0x7ff123456789);

        // 未命中
        cache.get(0x7ff00000000);

        let stats = cache.stats();
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert!((stats.hit_rate - 0.666).abs() < 0.001);
    }

    #[test]
    fn test_cache_invalidation() {
        let cache = SymbolCache::new();

        // 插入一些符号
        cache.insert(0x7ff10000100, create_test_symbol(0x7ff10000100, "Func1")).unwrap();
        cache.insert(0x7ff10000200, create_test_symbol(0x7ff10000200, "Func2")).unwrap();
        cache.insert(0x7ff20000100, create_test_symbol(0x7ff20000100, "Func3")).unwrap();

        // 缓存模块信息
        let module = ModuleInfo::new(0x7ff10000000, 0x10000, "test.dll");
        cache.cache_module(module).unwrap();

        // 使模块缓存失效
        cache.invalidate_module(0x7ff10000000).unwrap();

        // 检查模块范围内的符号已被移除
        assert!(cache.get(0x7ff10000100).is_none());
        assert!(cache.get(0x7ff10000200).is_none());

        // 其他模块的符号应该还在
        assert!(cache.get(0x7ff20000100).is_some());
    }

    #[test]
    fn test_cache_clear() {
        let cache = SymbolCache::new();

        cache.insert(0x7ff10000100, create_test_symbol(0x7ff10000100, "Func1")).unwrap();
        cache.insert(0x7ff10000200, create_test_symbol(0x7ff10000200, "Func2")).unwrap();

        cache.clear().unwrap();

        assert!(cache.is_empty());
        assert_eq!(cache.stats().entry_count, 0);
    }

    #[test]
    fn test_cache_stats() {
        let cache = SymbolCache::new();
        let symbol = create_test_symbol(0x7ff123456789, "TestFunction");

        cache.insert(0x7ff123456789, symbol).unwrap();
        cache.get(0x7ff123456789);

        let stats = cache.stats();
        assert_eq!(stats.entry_count, 1);
        assert_eq!(stats.inserts, 1);
        assert_eq!(stats.hits, 1);
    }

    #[test]
    fn test_module_cache() {
        let cache = SymbolCache::new();

        let module = ModuleInfo::new(0x7ff10000000, 0x10000, "test.dll");
        cache.cache_module(module.clone()).unwrap();

        let retrieved = cache.get_module(0x7ff10000000);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "test.dll");

        let found = cache.find_module_for_address(0x7ff10000500);
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "test.dll");
    }

    #[test]
    fn test_shared_symbol_cache() {
        let cache1 = SharedSymbolCache::new();
        let cache2 = cache1.clone();

        let symbol = create_test_symbol(0x7ff123456789, "TestFunction");
        cache1.insert(0x7ff123456789, symbol).unwrap();

        let retrieved = cache2.get(0x7ff123456789);
        assert!(retrieved.is_some());
    }
}
