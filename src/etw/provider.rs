//! ETW 提供者定义模块
//!
//! 定义内核提供者标志、事件 GUID 和提供者配置结构。
//! 使用 `bitflags` crate 实现高效的标志位操作。

use crate::error::{EtwError, Result};
use std::fmt;
use windows::core::GUID;

// ============================================================================
// 内核提供者标志位
// ============================================================================

macro_rules! define_kernel_flags {
    (
        $(#[$outer:meta])*
        pub struct $name:ident: $t:ty {
            $(
                $(#[$inner:meta])*
                const $const_name:ident = $value:expr;
            )*
        }
    ) => {
        $(#[$outer])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name {
            bits: $t,
        }

        impl $name {
            $(
                $(#[$inner])*
                pub const $const_name: Self = Self { bits: $value };
            )*

            /// 返回原始位值
            pub const fn bits(&self) -> $t {
                self.bits
            }

            /// 从原始位值创建
            pub const fn from_bits(bits: $t) -> Option<Self> {
                let truncated = Self::from_bits_truncate(bits);
                if truncated.bits == bits {
                    Some(truncated)
                } else {
                    None
                }
            }

            /// 从原始位值创建（截断未知位）
            pub const fn from_bits_truncate(bits: $t) -> Self {
                Self { bits }
            }

            /// 检查是否包含所有指定标志
            pub const fn contains(&self, other: Self) -> bool {
                (self.bits & other.bits) == other.bits
            }

            /// 检查是否为空
            pub const fn is_empty(&self) -> bool {
                self.bits == 0
            }

            /// 返回空标志
            pub const fn empty() -> Self {
                Self { bits: 0 }
            }

            /// 返回所有标志
            pub const fn all() -> Self {
                Self { bits: !0 }
            }
        }

        impl std::ops::BitOr for $name {
            type Output = Self;

            fn bitor(self, rhs: Self) -> Self::Output {
                Self { bits: self.bits | rhs.bits }
            }
        }

        impl std::ops::BitOrAssign for $name {
            fn bitor_assign(&mut self, rhs: Self) {
                self.bits |= rhs.bits;
            }
        }

        impl std::ops::BitAnd for $name {
            type Output = Self;

            fn bitand(self, rhs: Self) -> Self::Output {
                Self { bits: self.bits & rhs.bits }
            }
        }

        impl std::ops::BitAndAssign for $name {
            fn bitand_assign(&mut self, rhs: Self) {
                self.bits &= rhs.bits;
            }
        }

        impl std::ops::BitXor for $name {
            type Output = Self;

            fn bitxor(self, rhs: Self) -> Self::Output {
                Self { bits: self.bits ^ rhs.bits }
            }
        }

        impl std::ops::Not for $name {
            type Output = Self;

            fn not(self) -> Self::Output {
                Self { bits: !self.bits }
            }
        }

    };
}

define_kernel_flags! {
    /// 内核提供者标志位
    ///
    /// 定义要启用的内核 ETW 提供者事件类型。这些标志对应于
    /// Windows 内核事件提供者的不同事件类别。
    ///
    /// # 示例
    ///
    /// ```
    /// use etw_profiler::etw::KernelProviderFlags;
    ///
    /// // 启用性能分析和进程线程事件
    /// let flags = KernelProviderFlags::PROFILE | KernelProviderFlags::PROC_THREAD;
    ///
    /// // 启用所有常用事件
    /// let all_flags = KernelProviderFlags::all();
    /// ```
    pub struct KernelProviderFlags: u32 {
        /// 进程生命周期事件（启动、终止）
        ///
        /// 启用 ProcessStart 和 ProcessStop 事件
        const PROC_THREAD = 0x00000001;

        /// 内存页错误事件
        ///
        /// 硬页错误和软页错误
        const MEMORY = 0x00000002;

        /// 磁盘 I/O 事件
        ///
        /// 磁盘读取和写入操作
        const DISK_IO = 0x00000100;

        /// 磁盘 I/O 完成事件
        ///
        /// 磁盘操作完成通知
        const DISK_IO_INIT = 0x00000200;

        /// 高性能磁盘 I/O 事件
        ///
        /// 用于详细磁盘性能分析
        const DISK_IO_HP = 0x00000400;

        /// 网络 TCP/IP 事件
        ///
        /// TCP 连接和数据传输事件
        const NETWORK_TCPIP = 0x00010000;

        /// 注册表访问事件
        ///
        /// 注册表读取和写入操作
        const REGISTRY = 0x00020000;

        /// 告警事件
        ///
        /// 系统告警和通知
        const ALPC = 0x00100000;

        /// 进程计数器
        ///
        /// 进程性能计数器
        const PROCESS_COUNTERS = 0x00000008;

        /// 上下文切换事件
        ///
        /// 线程上下文切换
        const CSWITCH = 0x00000010;

        /// 延迟过程调用 (DPC) 事件
        ///
        /// 内核 DPC 执行
        const DPC = 0x00000020;

        /// 中断事件
        ///
        /// 硬件中断处理
        const INTERRUPT = 0x00000040;

        /// 系统调用事件
        ///
        /// 用户态到内核态的系统调用
        const SYSTEMCALL = 0x00000080;

        /// 磁盘文件 I/O 事件
        ///
        /// 文件系统层面的 I/O 操作
        const DISK_FILE_IO = 0x00000800;

        /// 文件 I/O 事件
        ///
        /// 文件操作（创建、读取、写入、关闭）
        const FILE_IO = 0x00001000;

        /// 文件 I/O 完成事件
        ///
        /// 文件操作完成通知
        const FILE_IO_INIT = 0x00002000;

        /// 映射文件 I/O 事件
        ///
        /// 内存映射文件操作
        const MAPPED_IO = 0x00004000;

        /// 硬页错误事件
        ///
        /// 需要从磁盘加载页面的页错误
        const HARD_FAULTS = 0x00008000;

        /// 映像加载事件
        ///
        /// DLL 和 EXE 加载/卸载事件
        const IMAGE_LOAD = 0x00040000;

        /// 性能分析采样事件（核心）
        ///
        /// CPU 采样配置文件，用于热点分析
        /// 这是最常用的标志，用于捕获执行栈采样
        const PROFILE = 0x01000000;

        /// 线程调度事件
        ///
        /// 详细的线程调度信息
        const THREAD_SCHEDULING = 0x02000000;

        /// 跟踪转储事件
        ///
        /// 用于转储跟踪数据
        const TRACE_META = 0x04000000;

        /// 内存页错误事件（详细）
        ///
        /// 详细的页错误信息
        const PAGE_FAULTS = 0x08000000;

        /// 对象引用事件
        ///
        /// 内核对象引用跟踪
        const OBJECT_REF = 0x10000000;

        /// 电源管理事件
        ///
        /// 系统电源状态变化
        const POWER = 0x20000000;

        /// 模块加载/卸载事件（详细）
        ///
        /// 详细的模块加载信息
        const MODULE_LOAD = 0x40000000;

        /// 浮点异常事件
        ///
        /// 浮点运算异常
        const FLOATING_POINT = 0x80000000;
    }
}

impl KernelProviderFlags {
    /// 创建用于 CPU 性能分析的标志组合
    ///
    /// 包含采样、进程/线程、模块加载等必要事件
    pub fn profiling() -> Self {
        Self::PROFILE
            | Self::PROC_THREAD
            | Self::IMAGE_LOAD
            | Self::THREAD_SCHEDULING
            | Self::CSWITCH
    }

    /// 创建用于 I/O 分析的标志组合
    ///
    /// 包含磁盘和网络 I/O 事件
    pub fn io_analysis() -> Self {
        Self::DISK_IO | Self::DISK_IO_INIT | Self::FILE_IO | Self::NETWORK_TCPIP
    }

    /// 创建用于内存分析的标志组合
    ///
    /// 包含页错误和内存分配事件
    pub fn memory_analysis() -> Self {
        Self::MEMORY | Self::HARD_FAULTS | Self::PAGE_FAULTS | Self::MAPPED_IO
    }

    /// 创建包含所有常用事件的标志组合
    ///
    /// 用于全面的系统分析（性能开销较大）
    pub fn comprehensive() -> Self {
        Self::PROFILE
            | Self::PROC_THREAD
            | Self::IMAGE_LOAD
            | Self::DISK_IO
            | Self::FILE_IO
            | Self::NETWORK_TCPIP
            | Self::MEMORY
            | Self::CSWITCH
    }

    /// 获取用于 ETW 会话的原始标志值
    ///
    /// 这个值可以直接传递给 Windows ETW API
    pub fn to_raw(&self) -> u32 {
        self.bits()
    }

    /// 从原始标志值创建
    ///
    /// # 参数
    ///
    /// - `bits`: 原始标志位
    ///
    /// # 返回
    ///
    /// 解析后的标志位（可能包含未知位）
    pub fn from_raw(bits: u32) -> Self {
        Self::from_bits_truncate(bits)
    }

    /// 获取标志位的描述文本
    pub fn description(&self) -> Vec<&'static str> {
        let mut descs = Vec::new();
        if self.contains(Self::PROFILE) {
            descs.push("Sampling Profile");
        }
        if self.contains(Self::PROC_THREAD) {
            descs.push("Process/Thread");
        }
        if self.contains(Self::IMAGE_LOAD) {
            descs.push("Image Load");
        }
        if self.contains(Self::DISK_IO) {
            descs.push("Disk I/O");
        }
        if self.contains(Self::FILE_IO) {
            descs.push("File I/O");
        }
        if self.contains(Self::NETWORK_TCPIP) {
            descs.push("Network TCP/IP");
        }
        if self.contains(Self::MEMORY) {
            descs.push("Memory");
        }
        if self.contains(Self::SYSTEMCALL) {
            descs.push("System Call");
        }
        if self.contains(Self::CSWITCH) {
            descs.push("Context Switch");
        }
        if self.contains(Self::REGISTRY) {
            descs.push("Registry");
        }
        descs
    }
}

impl fmt::Display for KernelProviderFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let descs = self.description();
        if descs.is_empty() {
            write!(f, "None")
        } else {
            write!(f, "{}", descs.join(", "))
        }
    }
}

impl Default for KernelProviderFlags {
    fn default() -> Self {
        Self::profiling()
    }
}

// ============================================================================
// 内核提供者 GUID 常量
// ============================================================================

/// Windows 内核事件提供者 GUID
///
/// 这是系统内核事件提供者的 GUID，用于启用内核事件跟踪
pub const KERNEL_PROVIDER_GUID: GUID = GUID::from_values(
    0x9E814AAD,
    0x3204,
    0x11D2,
    [0x9A, 0x82, 0x00, 0x60, 0x08, 0xA8, 0x69, 0x39],
);

/// SampledProfile 事件 GUID
///
/// CPU 采样配置文件事件的 GUID
pub const SAMPLED_PROFILE_GUID: GUID = GUID::from_values(
    0xce1dbfb4,
    0x137e,
    0x4da6,
    [0x87, 0xb0, 0x3f, 0x59, 0xaa, 0x10, 0x2c, 0xbc],
);

/// 进程事件 GUID
///
/// 进程启动和停止事件的 GUID
pub const PROCESS_GUID: GUID = GUID::from_values(
    0x3d6fa8d1,
    0xfe05,
    0x11d0,
    [0x9d, 0xda, 0x00, 0xc0, 0x4f, 0xd7, 0xba, 0x7c],
);

/// 线程事件 GUID
///
/// 线程启动和停止事件的 GUID
pub const THREAD_GUID: GUID = GUID::from_values(
    0x3d6fa8d0,
    0xfe05,
    0x11d0,
    [0x9d, 0xda, 0x00, 0xc0, 0x4f, 0xd7, 0xba, 0x7c],
);

/// 映像加载事件 GUID
///
/// DLL 和 EXE 加载事件的 GUID
pub const IMAGE_LOAD_GUID: GUID = GUID::from_values(
    0x2cb15d1d,
    0x5fc1,
    0x11d2,
    [0xab, 0xe8, 0x00, 0x90, 0x27, 0x60, 0xb7, 0x1e],
);

/// 磁盘 I/O 事件 GUID
///
/// 磁盘操作事件的 GUID
pub const DISK_IO_GUID: GUID = GUID::from_values(
    0x3d6fa8c4,
    0xfe05,
    0x11d0,
    [0x9d, 0xda, 0x00, 0xc0, 0x4f, 0xd7, 0xba, 0x7c],
);

/// 文件 I/O 事件 GUID
///
/// 文件操作事件的 GUID
pub const FILE_IO_GUID: GUID = GUID::from_values(
    0x90cbdc39,
    0x4a3e,
    0x11d1,
    [0x84, 0xf4, 0x00, 0x00, 0xf8, 0x04, 0x64, 0xe3],
);

/// 注册表访问事件 GUID
///
/// 注册表操作事件的 GUID
pub const REGISTRY_GUID: GUID = GUID::from_values(
    0xAE53722E,
    0xC863,
    0x11d2,
    [0x86, 0x59, 0x00, 0xC0, 0x4F, 0xA3, 0x21, 0xA1],
);

/// TCP/IP 网络事件 GUID
///
/// 网络事件提供者的 GUID
pub const TCPIP_GUID: GUID = GUID::from_values(
    0x9A280AC0,
    0xC8E0,
    0x11D1,
    [0x84, 0xE2, 0x00, 0xC0, 0x4F, 0xB9, 0x98, 0xA2],
);

/// 页面错误事件 GUID
///
/// 内存页错误事件的 GUID
pub const PAGE_FAULT_GUID: GUID = GUID::from_values(
    0x3d6fa8d3,
    0xfe05,
    0x11d0,
    [0x9d, 0xda, 0x00, 0xc0, 0x4f, 0xd7, 0xba, 0x7c],
);

/// 上下文切换事件 GUID
///
/// 线程上下文切换事件的 GUID
pub const CSWITCH_GUID: GUID = GUID::from_values(
    0x3d6fa8d2,
    0xfe05,
    0x11d0,
    [0x9d, 0xda, 0x00, 0xc0, 0x4f, 0xd7, 0xba, 0x7c],
);

/// 系统调用事件 GUID
///
/// 系统调用事件的 GUID
pub const SYSCALL_GUID: GUID = GUID::from_values(
    0x4343449D,
    0x8E1E,
    0x46D4,
    [0x82, 0xDD, 0xD3, 0x20, 0x82, 0x95, 0x0F, 0x17],
);

// ============================================================================
// 提供者配置结构
// ============================================================================

/// 提供者配置结构
///
/// 用于配置单个 ETW 提供者的参数
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderConfig {
    /// 提供者 GUID
    pub guid: GUID,
    /// 启用标志位
    pub enable_flags: u32,
    /// 日志级别 (0-255)
    pub level: u8,
    /// 属性标志
    pub property_flags: u32,
    /// 匹配任何关键字
    pub match_any_keyword: u64,
    /// 匹配所有关键字
    pub match_all_keyword: u64,
    /// 超时时间（毫秒）
    pub timeout_ms: u32,
}

impl ProviderConfig {
    /// 创建新的提供者配置
    ///
    /// # 参数
    ///
    /// - `guid`: 提供者 GUID
    pub fn new(guid: GUID) -> Self {
        Self {
            guid,
            enable_flags: 0,
            level: 0,
            property_flags: 0,
            match_any_keyword: 0,
            match_all_keyword: 0,
            timeout_ms: 0,
        }
    }

    /// 创建内核提供者配置
    ///
    /// # 参数
    ///
    /// - `flags`: 内核提供者标志
    pub fn kernel(flags: KernelProviderFlags) -> Self {
        Self {
            guid: KERNEL_PROVIDER_GUID,
            enable_flags: flags.to_raw(),
            level: 0,
            property_flags: 0,
            match_any_keyword: 0,
            match_all_keyword: 0,
            timeout_ms: 0,
        }
    }

    /// 设置启用标志
    pub fn with_flags(mut self, flags: u32) -> Self {
        self.enable_flags = flags;
        self
    }

    /// 设置日志级别
    pub fn with_level(mut self, level: u8) -> Self {
        self.level = level;
        self
    }

    /// 设置匹配关键字
    pub fn with_keywords(mut self, any: u64, all: u64) -> Self {
        self.match_any_keyword = any;
        self.match_all_keyword = all;
        self
    }

    /// 设置超时时间
    pub fn with_timeout(mut self, timeout_ms: u32) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// 验证配置是否有效
    pub fn validate(&self) -> Result<()> {
        // GUID 不能全为零
        let is_zero_guid = self.guid.data1 == 0
            && self.guid.data2 == 0
            && self.guid.data3 == 0
            && self.guid.data4 == [0; 8];

        if is_zero_guid {
            return Err(EtwError::new("Invalid provider GUID: all zeros").into());
        }

        Ok(())
    }
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self::kernel(KernelProviderFlags::default())
    }
}

// ============================================================================
// 事件类型常量
// ============================================================================

/// 事件类型：进程启动
pub const EVENT_TYPE_PROCESS_START: u16 = 1;

/// 事件类型：进程终止
pub const EVENT_TYPE_PROCESS_END: u16 = 2;

/// 事件类型：线程启动
pub const EVENT_TYPE_THREAD_START: u16 = 1;

/// 事件类型：线程终止
pub const EVENT_TYPE_THREAD_END: u16 = 2;

/// 事件类型：映像加载
pub const EVENT_TYPE_IMAGE_LOAD: u16 = 10;

/// 事件类型：映像卸载
pub const EVENT_TYPE_IMAGE_UNLOAD: u16 = 11;

/// 事件类型：采样配置文件
pub const EVENT_TYPE_SAMPLED_PROFILE: u16 = 46;

/// 事件类型：上下文切换
pub const EVENT_TYPE_CSWITCH: u16 = 36;

// ============================================================================
// 版本信息
// ============================================================================

/// 内核提供者版本
pub const KERNEL_PROVIDER_VERSION: u32 = 2;

/// 默认日志级别
pub const DEFAULT_LOG_LEVEL: u8 = 5;

// ============================================================================
// 测试模块
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kernel_provider_flags() {
        let flags = KernelProviderFlags::PROFILE | KernelProviderFlags::PROC_THREAD;
        assert!(flags.contains(KernelProviderFlags::PROFILE));
        assert!(flags.contains(KernelProviderFlags::PROC_THREAD));
        assert!(!flags.contains(KernelProviderFlags::DISK_IO));

        let raw = flags.to_raw();
        let parsed = KernelProviderFlags::from_raw(raw);
        assert_eq!(flags.bits(), parsed.bits());
    }

    #[test]
    fn test_kernel_provider_flags_profiling() {
        let flags = KernelProviderFlags::profiling();
        assert!(flags.contains(KernelProviderFlags::PROFILE));
        assert!(flags.contains(KernelProviderFlags::PROC_THREAD));
        assert!(flags.contains(KernelProviderFlags::IMAGE_LOAD));
    }

    #[test]
    fn test_kernel_provider_flags_io_analysis() {
        let flags = KernelProviderFlags::io_analysis();
        assert!(flags.contains(KernelProviderFlags::DISK_IO));
        assert!(flags.contains(KernelProviderFlags::FILE_IO));
        assert!(flags.contains(KernelProviderFlags::NETWORK_TCPIP));
    }

    #[test]
    fn test_kernel_provider_description() {
        let flags = KernelProviderFlags::PROFILE | KernelProviderFlags::PROC_THREAD;
        let desc = flags.description();
        assert!(desc.contains(&"Sampling Profile"));
        assert!(desc.contains(&"Process/Thread"));
    }

    #[test]
    fn test_provider_config() {
        let config = ProviderConfig::kernel(KernelProviderFlags::PROFILE)
            .with_level(5)
            .with_timeout(1000);

        assert_eq!(config.guid, KERNEL_PROVIDER_GUID);
        assert!(config.enable_flags & KernelProviderFlags::PROFILE.bits() != 0);
        assert_eq!(config.level, 5);
        assert_eq!(config.timeout_ms, 1000);

        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_provider_config_validation() {
        let zero_guid = GUID::from_values(0, 0, 0, [0; 8]);
        let config = ProviderConfig::new(zero_guid);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_kernel_provider_display() {
        let flags = KernelProviderFlags::PROFILE;
        let display = format!("{}", flags);
        assert!(display.contains("Sampling Profile"));

        let empty = KernelProviderFlags::empty();
        assert_eq!(format!("{}", empty), "None");
    }
}
