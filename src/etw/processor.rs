//! ETW 事件处理器模块
//!
//! 该模块负责处理从 ETW 会话接收的原始事件记录，将其转换为
//! 应用程序友好的数据结构（如 `SampleEvent`），并通过回调
//! 机制传递给上层模块。
//!
//! # 核心功能
//!
//! - 解析 `SampledProfile` 事件（CPU 采样）
//! - 跟踪进程/线程生命周期事件
//! - 处理模块加载事件（用于符号解析）
//! - 将原始事件转换为 `SampleEvent` 类型
//!
//! # 线程安全
//!
//! `EventProcessor` 实现了 `Send` 和 `Sync`，可以在多线程环境中安全使用

use crate::error::Result;
use crate::etw::{
    EventHandler, SampledProfileEvent, safe_utf16_to_string,
};
use crate::etw::provider::{
    EVENT_TYPE_PROCESS_START, EVENT_TYPE_PROCESS_END,
    EVENT_TYPE_THREAD_START, EVENT_TYPE_THREAD_END,
    EVENT_TYPE_IMAGE_LOAD, EVENT_TYPE_IMAGE_UNLOAD,
    EVENT_TYPE_SAMPLED_PROFILE,
    CSWITCH_GUID, IMAGE_LOAD_GUID, KERNEL_PROVIDER_GUID, 
    PAGE_FAULT_GUID, PROCESS_GUID, REGISTRY_GUID, 
    SAMPLED_PROFILE_GUID, SYSCALL_GUID, TCPIP_GUID, THREAD_GUID,
};
use crate::types::{Address, ProcessId, SampleEvent, ThreadId, Timestamp};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use tracing::{debug, error, trace, warn};
use windows::core::GUID;
use windows::Win32::System::Diagnostics::Etw::EVENT_RECORD;

// ============================================================================
// 进程/线程上下文跟踪
// ============================================================================

/// 进程上下文信息
#[derive(Debug, Clone)]
pub struct ProcessContext {
    /// 进程 ID
    pub pid: ProcessId,
    /// 进程名称
    pub name: String,
    /// 命令行
    pub command_line: Option<String>,
    /// 父进程 ID
    pub parent_pid: Option<ProcessId>,
    /// 开始时间戳
    pub start_time: Timestamp,
    /// 结束时间戳
    pub end_time: Option<Timestamp>,
    /// 是否已结束
    pub is_alive: bool,
}

impl ProcessContext {
    pub fn new(pid: ProcessId, name: impl Into<String>, start_time: Timestamp) -> Self {
        Self {
            pid,
            name: name.into(),
            command_line: None,
            parent_pid: None,
            start_time,
            end_time: None,
            is_alive: true,
        }
    }

    pub fn with_command_line(mut self, cmd: impl Into<String>) -> Self {
        self.command_line = Some(cmd.into());
        self
    }

    pub fn with_parent(mut self, parent_pid: ProcessId) -> Self {
        self.parent_pid = Some(parent_pid);
        self
    }

    pub fn mark_ended(&mut self, end_time: Timestamp) {
        self.end_time = Some(end_time);
        self.is_alive = false;
    }
}

/// 线程上下文信息
#[derive(Debug, Clone)]
pub struct ThreadContext {
    /// 线程 ID
    pub tid: ThreadId,
    /// 所属进程 ID
    pub pid: ProcessId,
    /// 开始时间戳
    pub start_time: Timestamp,
    /// 结束时间戳
    pub end_time: Option<Timestamp>,
    /// 是否存活
    pub is_alive: bool,
}

impl ThreadContext {
    pub fn new(tid: ThreadId, pid: ProcessId, start_time: Timestamp) -> Self {
        Self {
            tid,
            pid,
            start_time,
            end_time: None,
            is_alive: true,
        }
    }

    pub fn mark_ended(&mut self, end_time: Timestamp) {
        self.end_time = Some(end_time);
        self.is_alive = false;
    }
}

/// 模块加载信息
#[derive(Debug, Clone)]
pub struct ModuleInfo {
    /// 模块基地址
    pub base_address: Address,
    /// 模块大小
    pub size: u64,
    /// 模块名称
    pub name: String,
    /// 完整路径
    pub path: Option<String>,
    /// 时间戳
    pub timestamp: u64,
}

impl ModuleInfo {
    pub fn new(base_address: Address, size: u64, name: impl Into<String>) -> Self {
        Self {
            base_address,
            size,
            name: name.into(),
            path: None,
            timestamp: 0,
        }
    }

    pub fn contains_address(&self, address: Address) -> bool {
        address >= self.base_address && address < self.base_address + self.size
    }
}

/// 上下文跟踪器
///
/// 维护进程、线程和模块的上下文信息
#[derive(Debug)]
pub struct ContextTracker {
    /// 进程上下文映射
    processes: HashMap<ProcessId, ProcessContext>,
    /// 线程上下文映射
    threads: HashMap<ThreadId, ThreadContext>,
    /// 模块信息映射 (pid -> (base_address -> ModuleInfo))
    modules: HashMap<ProcessId, HashMap<Address, ModuleInfo>>,
}

impl ContextTracker {
    pub fn new() -> Self {
        Self {
            processes: HashMap::new(),
            threads: HashMap::new(),
            modules: HashMap::new(),
        }
    }

    pub fn add_process(&mut self, context: ProcessContext) {
        trace!("Adding process context: PID={}, Name={}", context.pid, context.name);
        self.processes.insert(context.pid, context);
    }

    pub fn remove_process(&mut self, pid: ProcessId, end_time: Timestamp) {
        if let Some(ctx) = self.processes.get_mut(&pid) {
            ctx.mark_ended(end_time);
            trace!("Marked process as ended: PID={}", pid);
        }
    }

    pub fn get_process(&self, pid: ProcessId) -> Option<&ProcessContext> {
        self.processes.get(&pid)
    }

    pub fn add_thread(&mut self, context: ThreadContext) {
        trace!("Adding thread context: TID={}, PID={}", context.tid, context.pid);
        self.threads.insert(context.tid, context);
    }

    pub fn remove_thread(&mut self, tid: ThreadId, end_time: Timestamp) {
        if let Some(ctx) = self.threads.get_mut(&tid) {
            ctx.mark_ended(end_time);
            trace!("Marked thread as ended: TID={}", tid);
        }
    }

    pub fn get_thread(&self, tid: ThreadId) -> Option<&ThreadContext> {
        self.threads.get(&tid)
    }

    pub fn get_thread_process(&self, tid: ThreadId) -> Option<ProcessId> {
        self.threads.get(&tid).map(|ctx| ctx.pid)
    }

    pub fn add_module(&mut self, pid: ProcessId, info: ModuleInfo) {
        let module_map = self.modules.entry(pid).or_insert_with(HashMap::new);
        trace!(
            "Adding module: PID={}, Name={}, Base=0x{:016X}, Size={}",
            pid, info.name, info.base_address, info.size
        );
        module_map.insert(info.base_address, info);
    }

    pub fn remove_module(&mut self, pid: ProcessId, base_address: Address) {
        if let Some(module_map) = self.modules.get_mut(&pid) {
            if let Some(info) = module_map.remove(&base_address) {
                trace!("Removed module: PID={}, Name={}", pid, info.name);
            }
        }
    }

    pub fn get_module_for_address(&self, pid: ProcessId, address: Address) -> Option<&ModuleInfo> {
        self.modules.get(&pid).and_then(|module_map| {
            module_map.values().find(|info| info.contains_address(address))
        })
    }

    pub fn get_module_by_name(&self, pid: ProcessId, name: &str) -> Option<&ModuleInfo> {
        self.modules.get(&pid).and_then(|module_map| {
            module_map.values().find(|info| info.name == name)
        })
    }

    pub fn process_count(&self) -> usize {
        self.processes.len()
    }

    pub fn thread_count(&self) -> usize {
        self.threads.len()
    }

    pub fn module_count(&self, pid: Option<ProcessId>) -> usize {
        match pid {
            Some(p) => self.modules.get(&p).map(|m| m.len()).unwrap_or(0),
            None => self.modules.values().map(|m| m.len()).sum(),
        }
    }
}

impl Default for ContextTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 处理后的事件枚举
// ============================================================================

/// 处理后的事件类型
#[derive(Debug, Clone)]
pub enum ProcessedEvent {
    /// CPU 采样事件
    Sample(SampledProfileEvent),
    /// 进程开始
    ProcessStart(ProcessContext),
    /// 进程结束
    ProcessEnd(ProcessId, Timestamp, u32),
    /// 线程开始
    ThreadStart(ThreadContext),
    /// 线程结束
    ThreadEnd(ThreadId, Timestamp),
    /// 模块加载
    ImageLoad(ProcessId, ModuleInfo),
    /// 模块卸载
    ImageUnload(ProcessId, Address),
    /// 上下文切换
    ContextSwitch {
        timestamp: Timestamp,
        old_tid: ThreadId,
        new_tid: ThreadId,
        old_priority: u8,
        new_priority: u8,
    },
    /// 系统调用
    Syscall {
        timestamp: Timestamp,
        pid: ProcessId,
        tid: ThreadId,
        syscall_id: u32,
    },
    /// 页错误
    PageFault {
        timestamp: Timestamp,
        pid: ProcessId,
        tid: ThreadId,
        address: Address,
    },
    /// 原始事件（未处理的）
    Raw(*const EVENT_RECORD),
}

// ============================================================================
// 事件处理器
// ============================================================================

/// 事件处理回调函数类型
pub type EventCallback = Box<dyn Fn(&ProcessedEvent) + Send + Sync>;

/// ETW 事件处理器
///
/// 负责解析 ETW 事件记录并将其转换为应用程序友好的格式。
/// 支持自定义事件回调和上下文跟踪。
pub struct EventProcessor {
    /// 上下文跟踪器
    context_tracker: Arc<RwLock<ContextTracker>>,
    /// 事件回调列表
    callbacks: Vec<EventCallback>,
    /// 采样事件计数
    sample_count: Arc<Mutex<u64>>,
    /// 处理的原始事件计数
    raw_event_count: Arc<Mutex<u64>>,
}

impl EventProcessor {
    /// 创建新的事件处理器
    pub fn new() -> Self {
        Self {
            context_tracker: Arc::new(RwLock::new(ContextTracker::new())),
            callbacks: Vec::new(),
            sample_count: Arc::new(Mutex::new(0)),
            raw_event_count: Arc::new(Mutex::new(0)),
        }
    }

    /// 添加事件回调
    pub fn add_callback<F>(&mut self, callback: F)
    where
        F: Fn(&ProcessedEvent) + Send + Sync + 'static,
    {
        self.callbacks.push(Box::new(callback));
    }

    /// 处理事件记录
    ///
    /// 这是主要的入口点，接收来自 ETW 的原始事件记录。
    ///
    /// # 安全
    ///
    /// 此方法内部使用 unsafe 代码来解析事件数据
    pub fn process_event(&self, event_record: *const EVENT_RECORD) {
        // 安全说明：我们需要访问原始事件记录指针
        let record = unsafe { &*event_record };
        
        // 更新计数
        {
            let mut count = self.raw_event_count.lock().unwrap();
            *count += 1;
        }

        // 提取事件头部信息
        let header = &record.EventHeader;
        let timestamp = self.convert_timestamp(header.TimeStamp as u64);
        let event_guid = &header.ProviderId;
        let event_type = header.EventDescriptor.Opcode;
        let process_id = header.ProcessId;
        let thread_id = header.ThreadId;

        trace!(
            "Processing event: GUID={:?}, Type={}, PID={}, TID={}",
            event_guid, event_type, process_id, thread_id
        );

        // 根据事件 GUID 分发处理
        let processed = if Self::is_kernel_provider(event_guid) {
            self.process_kernel_event(record, timestamp, event_type, process_id, thread_id)
        } else {
            Some(ProcessedEvent::Raw(event_record))
        };

        // 调用回调
        if let Some(event) = processed {
            for callback in &self.callbacks {
                callback(&event);
            }
        }
    }

    /// 检查是否为内核提供者事件
    fn is_kernel_provider(guid: &GUID) -> bool {
        guid == &KERNEL_PROVIDER_GUID
            || guid == &SAMPLED_PROFILE_GUID
            || guid == &PROCESS_GUID
            || guid == &THREAD_GUID
            || guid == &IMAGE_LOAD_GUID
            || guid == &CSWITCH_GUID
            || guid == &PAGE_FAULT_GUID
            || guid == &REGISTRY_GUID
            || guid == &TCPIP_GUID
            || guid == &SYSCALL_GUID
    }

    /// 处理内核事件
    fn process_kernel_event(
        &self,
        record: &EVENT_RECORD,
        timestamp: Timestamp,
        event_type: u8,
        process_id: u32,
        thread_id: u32,
    ) -> Option<ProcessedEvent> {
        // 将 u16 常量转换为 u8 进行比较
        match event_type {
            x if x == (EVENT_TYPE_SAMPLED_PROFILE as u8) => {
                self.process_sampled_profile(record, timestamp, process_id, thread_id)
            }
            x if x == (EVENT_TYPE_PROCESS_START as u8) => {
                self.process_process_start(record, timestamp)
            }
            x if x == (EVENT_TYPE_PROCESS_END as u8) => {
                self.process_process_end(record, timestamp)
            }
            x if x == (EVENT_TYPE_THREAD_START as u8) => {
                self.process_thread_start(record, timestamp)
            }
            x if x == (EVENT_TYPE_THREAD_END as u8) => {
                self.process_thread_end(record, timestamp)
            }
            x if x == (EVENT_TYPE_IMAGE_LOAD as u8) => {
                self.process_image_load(record, timestamp)
            }
            x if x == (EVENT_TYPE_IMAGE_UNLOAD as u8) => {
                self.process_image_unload(record, timestamp)
            }
            _ => {
                trace!("Unhandled kernel event type: {}", event_type);
                None
            }
        }
    }

    /// 处理采样配置文件事件
    fn process_sampled_profile(
        &self,
        record: &EVENT_RECORD,
        timestamp: Timestamp,
        process_id: u32,
        thread_id: u32,
    ) -> Option<ProcessedEvent> {
        // 安全说明：解析事件的用户数据
        unsafe {
            if record.UserData.is_null() || record.UserDataLength < 8 {
                warn!("Invalid sampled profile event data");
                return None;
            }

            // 读取指令指针（前 8 个字节）
            let instruction_pointer = *(record.UserData as *const u64);
            
            // 检测用户模式/内核模式
            let user_mode = instruction_pointer < 0x7FFF00000000;

            let event = SampledProfileEvent {
                timestamp,
                process_id,
                thread_id,
                instruction_pointer,
                user_mode,
                processor_core: None,
                priority: None,
            };

            // 更新采样计数
            {
                let mut count = self.sample_count.lock().unwrap();
                *count += 1;
            }

            debug!(
                "SampledProfile: PID={}, TID={}, IP=0x{:016X}, UserMode={}",
                process_id, thread_id, instruction_pointer, user_mode
            );

            Some(ProcessedEvent::Sample(event))
        }
    }

    /// 处理进程开始事件
    fn process_process_start(
        &self,
        record: &EVENT_RECORD,
        timestamp: Timestamp,
    ) -> Option<ProcessedEvent> {
        unsafe {
            if record.UserData.is_null() {
                return None;
            }

            // 解析进程开始事件数据
            // 格式：ProcessId (4), ParentId (4), SessionId (4), ...
            let data = record.UserData as *const u32;
            let pid = *data;
            let parent_pid = *data.add(1);
            
            // 进程名称通常在扩展数据中
            let process_name = self.extract_process_name(record);

            let context = ProcessContext::new(pid, &process_name, timestamp)
                .with_parent(parent_pid);

            // 更新上下文跟踪器
            {
                let mut tracker = self.context_tracker.write().unwrap();
                tracker.add_process(context.clone());
            }

            debug!("Process started: PID={}, Name={}, Parent={}", pid, process_name, parent_pid);

            Some(ProcessedEvent::ProcessStart(context))
        }
    }

    /// 处理进程结束事件
    fn process_process_end(
        &self,
        record: &EVENT_RECORD,
        timestamp: Timestamp,
    ) -> Option<ProcessedEvent> {
        unsafe {
            if record.UserData.is_null() {
                return None;
            }

            let data = record.UserData as *const u32;
            let pid = *data;
            let exit_code = *data.add(4); // 退出代码位置

            // 更新上下文跟踪器
            {
                let mut tracker = self.context_tracker.write().unwrap();
                tracker.remove_process(pid, timestamp);
            }

            debug!("Process ended: PID={}, ExitCode={}", pid, exit_code);

            Some(ProcessedEvent::ProcessEnd(pid, timestamp, exit_code))
        }
    }

    /// 处理线程开始事件
    fn process_thread_start(
        &self,
        record: &EVENT_RECORD,
        timestamp: Timestamp,
    ) -> Option<ProcessedEvent> {
        unsafe {
            if record.UserData.is_null() {
                return None;
            }

            // 解析线程开始事件数据
            let data = record.UserData as *const u32;
            let tid = *data;
            let pid = *data.add(1);

            let context = ThreadContext::new(tid, pid, timestamp);

            // 更新上下文跟踪器
            {
                let mut tracker = self.context_tracker.write().unwrap();
                tracker.add_thread(context.clone());
            }

            trace!("Thread started: TID={}, PID={}", tid, pid);

            Some(ProcessedEvent::ThreadStart(context))
        }
    }

    /// 处理线程结束事件
    fn process_thread_end(
        &self,
        record: &EVENT_RECORD,
        timestamp: Timestamp,
    ) -> Option<ProcessedEvent> {
        unsafe {
            if record.UserData.is_null() {
                return None;
            }

            let data = record.UserData as *const u32;
            let tid = *data;

            // 更新上下文跟踪器
            {
                let mut tracker = self.context_tracker.write().unwrap();
                tracker.remove_thread(tid, timestamp);
            }

            trace!("Thread ended: TID={}", tid);

            Some(ProcessedEvent::ThreadEnd(tid, timestamp))
        }
    }

    /// 处理模块加载事件
    fn process_image_load(
        &self,
        record: &EVENT_RECORD,
        timestamp: Timestamp,
    ) -> Option<ProcessedEvent> {
        unsafe {
            if record.UserData.is_null() {
                return None;
            }

            // 解析模块加载事件
            // 格式：ImageBase (8), ImageSize (8), ProcessId (4), ...
            let data = record.UserData as *const u64;
            let base_address = *data;
            let size = *data.add(1);
            
            let pid_data = record.UserData.add(16) as *const u32;
            let pid = *pid_data;

            // 提取完整路径（而不仅仅是文件名）
            let module_path = self.extract_module_path(record);
            let module_name = self.extract_module_name(&module_path);

            // 创建 ModuleInfo 并设置完整路径
            let mut info = ModuleInfo::new(base_address, size, &module_name);
            info.path = Some(module_path.clone());
            
            // 更新上下文跟踪器
            {
                let mut tracker = self.context_tracker.write().unwrap();
                tracker.add_module(pid, info.clone());
            }

            debug!(
                "Image loaded: PID={}, Name={}, Path={}, Base=0x{:016X}, Size={}",
                pid, module_name, module_path, base_address, size
            );

            Some(ProcessedEvent::ImageLoad(pid, info))
        }
    }

    /// 处理模块卸载事件
    fn process_image_unload(
        &self,
        record: &EVENT_RECORD,
        timestamp: Timestamp,
    ) -> Option<ProcessedEvent> {
        unsafe {
            if record.UserData.is_null() {
                return None;
            }

            let data = record.UserData as *const u64;
            let base_address = *data;
            
            let pid_data = record.UserData.add(16) as *const u32;
            let pid = *pid_data;

            // 更新上下文跟踪器
            {
                let mut tracker = self.context_tracker.write().unwrap();
                tracker.remove_module(pid, base_address);
            }

            trace!("Image unloaded: PID={}, Base=0x{:016X}", pid, base_address);

            Some(ProcessedEvent::ImageUnload(pid, base_address))
        }
    }

    /// 从事件记录中提取进程名称
    unsafe fn extract_process_name(&self, record: &EVENT_RECORD) -> String {
        // 进程名称通常在扩展数据或用户数据的末尾
        // 这里简化处理，实际实现需要更复杂的解析
        if record.UserDataLength > 20 {
            let name_ptr = record.UserData.add(20) as *const u16;
            let max_len = (record.UserDataLength as usize - 20) / 2;
            let slice = std::slice::from_raw_parts(name_ptr, max_len.min(256));
            safe_utf16_to_string(slice)
        } else {
            String::from("<unknown>")
        }
    }

    /// 从事件记录中提取模块完整路径
    unsafe fn extract_module_path(&self, record: &EVENT_RECORD) -> String {
        // 模块路径通常在扩展数据或用户数据的末尾
        // 尝试从用户数据中提取 UTF-16 字符串
        if record.UserDataLength > 24 {
            let name_ptr = record.UserData.add(24) as *const u16;
            let max_len = (record.UserDataLength as usize - 24) / 2;
            let slice = std::slice::from_raw_parts(name_ptr, max_len.min(1024));
            let full_path = safe_utf16_to_string(slice);
            
            // 如果路径为空，尝试从文件名推断或使用默认值
            if full_path.is_empty() {
                return String::from("<unknown>");
            }
            
            full_path
        } else {
            String::from("<unknown>")
        }
    }

    /// 从完整路径中提取模块名称（文件名）
    fn extract_module_name(&self, path: &str) -> String {
        if path == "<unknown>" {
            return String::from("<unknown>");
        }
        
        // 从完整路径中提取文件名
        std::path::Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| path.to_string())
    }

    /// 转换时间戳
    ///
    /// 将 Windows QPC 时间戳转换为 Unix 微秒时间戳
    fn convert_timestamp(&self, timestamp: u64) -> Timestamp {
        // Windows 时间戳通常是 100ns 单位，从 1601-01-01 开始
        // 转换为 Unix 时间戳（1970-01-01 开始）的微妙
        
        // 从 1601 到 1970 的 100ns 间隔数
        const EPOCH_DIFFERENCE: u64 = 116_444_736_000_000_000;
        
        if timestamp > EPOCH_DIFFERENCE {
            let windows_time = timestamp - EPOCH_DIFFERENCE;
            // 转换为微秒 (100ns -> 1us)
            windows_time / 10
        } else {
            timestamp
        }
    }

    /// 获取上下文跟踪器
    pub fn context_tracker(&self) -> Arc<RwLock<ContextTracker>> {
        Arc::clone(&self.context_tracker)
    }

    /// 获取采样事件计数
    pub fn sample_count(&self) -> u64 {
        *self.sample_count.lock().unwrap()
    }

    /// 获取原始事件计数
    pub fn raw_event_count(&self) -> u64 {
        *self.raw_event_count.lock().unwrap()
    }

    /// 重置计数器
    pub fn reset_counters(&self) {
        *self.sample_count.lock().unwrap() = 0;
        *self.raw_event_count.lock().unwrap() = 0;
    }
}

impl Default for EventProcessor {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// EventHandler trait 实现
// ============================================================================

impl EventHandler for EventProcessor {
    fn on_sample(&mut self, event: &SampleEvent) {
        let profile_event = SampledProfileEvent::new(
            event.timestamp,
            event.process_id,
            event.thread_id,
            event.instruction_pointer,
        );

        let processed = ProcessedEvent::Sample(profile_event);
        for callback in &self.callbacks {
            callback(&processed);
        }
    }

    fn on_process_start(&mut self, timestamp: Timestamp, pid: ProcessId, name: Option<&str>) {
        let context = ProcessContext::new(
            pid,
            name.unwrap_or("<unknown>"),
            timestamp,
        );

        {
            let mut tracker = self.context_tracker.write().unwrap();
            tracker.add_process(context.clone());
        }

        let processed = ProcessedEvent::ProcessStart(context);
        for callback in &self.callbacks {
            callback(&processed);
        }
    }

    fn on_process_end(&mut self, timestamp: Timestamp, pid: ProcessId, exit_code: u32) {
        {
            let mut tracker = self.context_tracker.write().unwrap();
            tracker.remove_process(pid, timestamp);
        }

        let processed = ProcessedEvent::ProcessEnd(pid, timestamp, exit_code);
        for callback in &self.callbacks {
            callback(&processed);
        }
    }

    fn on_thread_start(&mut self, timestamp: Timestamp, pid: ProcessId, tid: ThreadId) {
        let context = ThreadContext::new(tid, pid, timestamp);

        {
            let mut tracker = self.context_tracker.write().unwrap();
            tracker.add_thread(context.clone());
        }

        let processed = ProcessedEvent::ThreadStart(context);
        for callback in &self.callbacks {
            callback(&processed);
        }
    }

    fn on_thread_end(&mut self, timestamp: Timestamp, pid: ProcessId, tid: ThreadId) {
        {
            let mut tracker = self.context_tracker.write().unwrap();
            tracker.remove_thread(tid, timestamp);
        }

        let processed = ProcessedEvent::ThreadEnd(tid, timestamp);
        for callback in &self.callbacks {
            callback(&processed);
        }
    }

    fn on_image_load(
        &mut self,
        timestamp: Timestamp,
        pid: ProcessId,
        base_address: Address,
        module_name: &str,
        size: u64,
        module_path: Option<&str>,
    ) {
        let mut info = ModuleInfo::new(base_address, size, module_name);
        info.path = module_path.map(|s| s.to_string());

        {
            let mut tracker = self.context_tracker.write().unwrap();
            tracker.add_module(pid, info.clone());
        }

        debug!(
            "EventHandler::on_image_load: PID={}, Name={}, Path={:?}, Base=0x{:016X}, Size={}",
            pid, module_name, module_path, base_address, size
        );

        let processed = ProcessedEvent::ImageLoad(pid, info);
        for callback in &self.callbacks {
            callback(&processed);
        }
    }

    fn on_syscall(
        &mut self,
        timestamp: Timestamp,
        pid: ProcessId,
        tid: ThreadId,
        syscall_id: u32,
    ) {
        let processed = ProcessedEvent::Syscall {
            timestamp,
            pid,
            tid,
            syscall_id,
        };
        for callback in &self.callbacks {
            callback(&processed);
        }
    }

    fn on_raw_event(&mut self, event_record: *const EVENT_RECORD) {
        self.process_event(event_record);
    }
}

// ============================================================================
// 测试模块
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_tracker() {
        let mut tracker = ContextTracker::new();

        // 添加进程
        let process = ProcessContext::new(1234, "test.exe", 1000);
        tracker.add_process(process);
        assert_eq!(tracker.process_count(), 1);

        // 添加线程
        let thread = ThreadContext::new(5678, 1234, 1000);
        tracker.add_thread(thread);
        assert_eq!(tracker.thread_count(), 1);

        // 添加模块
        let module = ModuleInfo::new(0x00400000, 0x10000, "test.exe");
        tracker.add_module(1234, module);
        assert_eq!(tracker.module_count(Some(1234)), 1);

        // 测试地址查找
        assert!(tracker.get_module_for_address(1234, 0x00401000).is_some());
        assert!(tracker.get_module_for_address(1234, 0x00500000).is_none());

        // 结束进程
        tracker.remove_process(1234, 2000);
        assert!(!tracker.get_process(1234).unwrap().is_alive);
    }

    #[test]
    fn test_event_processor() {
        let processor = EventProcessor::new();

        // 测试计数器初始值
        assert_eq!(processor.sample_count(), 0);
        assert_eq!(processor.raw_event_count(), 0);
    }

    #[test]
    fn test_module_contains_address() {
        let module = ModuleInfo::new(0x00400000, 0x10000, "test.exe");
        
        assert!(module.contains_address(0x00400000));
        assert!(module.contains_address(0x0040FFFF));
        assert!(!module.contains_address(0x003FFFFF));
        assert!(!module.contains_address(0x00500000));
    }

    #[test]
    fn test_timestamp_conversion() {
        let processor = EventProcessor::new();
        
        // Windows 时间戳 2024-01-01 00:00:00 UTC
        // 这是从 1601-01-01 开始的 100ns 间隔数
        let windows_timestamp: u64 = 133_494_912_000_000_000;
        let unix_us = processor.convert_timestamp(windows_timestamp);
        
        // Unix 时间戳 2024-01-01 00:00:00 UTC 的微秒数
        // 1704067200 * 1000000 = 1704067200000000
        let expected_unix_us: u64 = 1_704_067_200_000_000;
        
        assert_eq!(unix_us, expected_unix_us);
    }

    #[test]
    fn test_processed_event_variants() {
        let sample = SampledProfileEvent::new(1000, 1234, 5678, 0x00401000);
        let event = ProcessedEvent::Sample(sample.clone());
        
        match event {
            ProcessedEvent::Sample(e) => {
                assert_eq!(e.timestamp, 1000);
                assert_eq!(e.process_id, 1234);
            }
            _ => panic!("Expected Sample event"),
        }
    }
}
