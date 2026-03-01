//! 性能分析报告生成模块
//!
//! 提供专业的性能分析报告生成功能，专注于按用户需求的格式生成CSV报告。
//! 支持按线程分类的统计信息、函数调用堆栈、热点路径等多种报告类型。
//!
//! # 主要组件
//!
//! - [`ReportGenerator`]: 报告生成器 trait，定义报告生成的标准接口
//! - [`ReportConfig`]: 报告配置结构体，控制报告生成行为
//! - [`CsvReportGenerator`]: CSV报告生成器实现
//! - [`ReportFormatter`]: 报告格式化工具
//! - [`ReportSummary`]: 报告摘要信息
//! - [`ReportTemplate`]: 预定义的报告模板
//!
//! # 使用示例
//!
//! ```rust,no_run
//! use profiler::report::{CsvReportGenerator, ReportConfig, ReportGenerator, ReportTemplate};
//! use profiler::analysis::AnalysisResult;
//! use std::path::Path;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // 创建报告配置
//! let config = ReportConfig::default();
//! let generator = CsvReportGenerator::new(config);
//!
//! // 生成报告
//! // generator.generate(&analysis_result, Path::new("report.csv"))?;
//! # Ok(())
//! # }
//! ```

use crate::error::Result;
use crate::analysis::AnalysisResult;
use std::path::Path;

// 子模块定义
pub mod csv;
pub mod formatter;
pub mod summary;
pub mod templates;

// 公开导出
pub use csv::CsvReportGenerator;
pub use formatter::ReportFormatter;
pub use summary::{HotspotSummary, ReportSummary};
pub use templates::{ReportTemplate, TemplateConfig};

/// 报告生成器 Trait
///
/// 定义报告生成的标准接口，实现者可以提供不同的报告格式和策略。
///
/// # 线程安全
///
/// 该 trait 要求实现类型是 `Send + Sync` 的，以支持多线程报告生成。
pub trait ReportGenerator: Send + Sync {
    /// 生成报告到文件
    ///
    /// # 参数
    /// - `result`: 分析结果
    /// - `output_path`: 输出文件路径
    ///
    /// # 返回
    /// 生成成功返回 Ok(())，失败返回错误
    ///
    /// # 示例
    /// ```rust,ignore
    /// generator.generate(&result, Path::new("report.csv"))?;
    /// ```
    fn generate(&self, result: &AnalysisResult, output_path: &Path) -> Result<()>;

    /// 生成报告到字符串
    ///
    /// # 参数
    /// - `result`: 分析结果
    ///
    /// # 返回
    /// 包含报告内容的字符串
    fn generate_to_string(&self, result: &AnalysisResult) -> Result<String>;

    /// 获取报告配置
    fn config(&self) -> &ReportConfig;
}

/// 报告配置结构体
///
/// 控制报告生成的行为和格式选项。
#[derive(Debug, Clone)]
pub struct ReportConfig {
    /// 报告标题
    pub title: String,
    /// 是否包含UTF-8 BOM头（便于Excel识别）
    pub include_bom: bool,
    /// CSV分隔符
    pub delimiter: char,
    /// 是否包含表头
    pub include_headers: bool,
    /// 最大函数名长度
    pub max_function_name_length: usize,
    /// 最大文件路径长度
    pub max_file_path_length: usize,
    /// 调用堆栈最大深度
    pub max_stack_depth: usize,
    /// 热点路径最大数量
    pub max_hot_paths: usize,
    /// 是否包含系统函数
    pub include_system_functions: bool,
    /// 是否生成线程摘要
    pub include_thread_summary: bool,
    /// 是否生成热点路径
    pub include_hot_paths: bool,
    /// 是否生成调用堆栈详情
    pub include_call_stack_details: bool,
    /// 时间格式（strftime格式字符串）
    pub time_format: String,
    /// 浮点数精度
    pub float_precision: usize,
    /// 基础目录（用于相对路径计算）
    pub base_directory: Option<std::path::PathBuf>,
    /// 使用的报告模板
    pub template: ReportTemplate,
}

impl Default for ReportConfig {
    fn default() -> Self {
        Self {
            title: "ETW Performance Profiler Report".to_string(),
            include_bom: true,
            delimiter: ',',
            include_headers: true,
            max_function_name_length: 128,
            max_file_path_length: 256,
            max_stack_depth: 32,
            max_hot_paths: 100,
            include_system_functions: false,
            include_thread_summary: true,
            include_hot_paths: true,
            include_call_stack_details: false,
            time_format: "%Y-%m-%d %H:%M:%S".to_string(),
            float_precision: 2,
            base_directory: None,
            template: ReportTemplate::Detailed,
        }
    }
}

impl ReportConfig {
    /// 创建新的报告配置
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置报告标题
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// 设置是否包含BOM
    pub fn with_bom(mut self, include: bool) -> Self {
        self.include_bom = include;
        self
    }

    /// 设置分隔符
    pub fn with_delimiter(mut self, delimiter: char) -> Self {
        self.delimiter = delimiter;
        self
    }

    /// 设置是否包含表头
    pub fn with_headers(mut self, include: bool) -> Self {
        self.include_headers = include;
        self
    }

    /// 设置最大函数名长度
    pub fn with_max_function_name_length(mut self, length: usize) -> Self {
        self.max_function_name_length = length;
        self
    }

    /// 设置最大文件路径长度
    pub fn with_max_file_path_length(mut self, length: usize) -> Self {
        self.max_file_path_length = length;
        self
    }

    /// 设置最大堆栈深度
    pub fn with_max_stack_depth(mut self, depth: usize) -> Self {
        self.max_stack_depth = depth;
        self
    }

    /// 设置最大热点路径数量
    pub fn with_max_hot_paths(mut self, count: usize) -> Self {
        self.max_hot_paths = count;
        self
    }

    /// 设置是否包含系统函数
    pub fn with_system_functions(mut self, include: bool) -> Self {
        self.include_system_functions = include;
        self
    }

    /// 设置是否包含线程摘要
    pub fn with_thread_summary(mut self, include: bool) -> Self {
        self.include_thread_summary = include;
        self
    }

    /// 设置是否包含热点路径
    pub fn with_hot_paths(mut self, include: bool) -> Self {
        self.include_hot_paths = include;
        self
    }

    /// 设置是否包含调用堆栈详情
    pub fn with_call_stack_details(mut self, include: bool) -> Self {
        self.include_call_stack_details = include;
        self
    }

    /// 设置时间格式
    pub fn with_time_format(mut self, format: impl Into<String>) -> Self {
        self.time_format = format.into();
        self
    }

    /// 设置浮点数精度
    pub fn with_float_precision(mut self, precision: usize) -> Self {
        self.float_precision = precision;
        self
    }

    /// 设置基础目录
    pub fn with_base_directory(mut self, dir: impl AsRef<Path>) -> Self {
        self.base_directory = Some(dir.as_ref().to_path_buf());
        self
    }

    /// 设置报告模板
    pub fn with_template(mut self, template: ReportTemplate) -> Self {
        self.template = template;
        self.apply_template_settings();
        self
    }

    /// 应用模板预设配置
    fn apply_template_settings(&mut self) {
        match self.template {
            ReportTemplate::Detailed => {
                self.include_thread_summary = true;
                self.include_hot_paths = true;
                self.include_call_stack_details = true;
                self.max_hot_paths = 100;
            }
            ReportTemplate::Summary => {
                self.include_thread_summary = true;
                self.include_hot_paths = false;
                self.include_call_stack_details = false;
                self.max_hot_paths = 10;
            }
            ReportTemplate::ThreadFocused => {
                self.include_thread_summary = true;
                self.include_hot_paths = true;
                self.include_call_stack_details = false;
                self.max_hot_paths = 50;
            }
            ReportTemplate::FunctionFocused => {
                self.include_thread_summary = false;
                self.include_hot_paths = true;
                self.include_call_stack_details = true;
                self.max_hot_paths = 100;
            }
        }
    }

    /// 根据模板获取配置
    pub fn from_template(template: ReportTemplate) -> Self {
        let mut config = Self::default();
        config.template = template;
        config.apply_template_settings();
        config
    }
}

/// 报告章节类型
///
/// 定义报告中不同章节的类型标识。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportSection {
    /// 报告头部信息
    Header,
    /// 摘要信息
    Summary,
    /// 按线程分类的函数统计
    FunctionStatsByThread,
    /// 线程摘要统计
    ThreadSummary,
    /// 热点调用路径
    HotPaths,
    /// 调用堆栈详情
    CallStackDetails,
    /// 报告尾部
    Footer,
}

impl ReportSection {
    /// 获取章节的标题
    pub fn title(&self) -> &'static str {
        match self {
            ReportSection::Header => "Report Header",
            ReportSection::Summary => "Summary",
            ReportSection::FunctionStatsByThread => "Function Statistics by Thread",
            ReportSection::ThreadSummary => "Thread Summary",
            ReportSection::HotPaths => "Hot Call Paths",
            ReportSection::CallStackDetails => "Call Stack Details",
            ReportSection::Footer => "Report Footer",
        }
    }

    /// 获取章节的CSV注释标记
    pub fn comment_marker(&self) -> String {
        format!("## {}", self.title())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_report_config_default() {
        let config = ReportConfig::default();
        assert_eq!(config.title, "ETW Performance Profiler Report");
        assert!(config.include_bom);
        assert_eq!(config.delimiter, ',');
        assert!(config.include_headers);
        assert_eq!(config.max_function_name_length, 128);
    }

    #[test]
    fn test_report_config_builder() {
        let config = ReportConfig::new()
            .with_title("Custom Report")
            .with_bom(false)
            .with_delimiter(';')
            .with_max_function_name_length(64);

        assert_eq!(config.title, "Custom Report");
        assert!(!config.include_bom);
        assert_eq!(config.delimiter, ';');
        assert_eq!(config.max_function_name_length, 64);
    }

    #[test]
    fn test_report_template_config() {
        let config = ReportConfig::from_template(ReportTemplate::Summary);
        assert!(config.include_thread_summary);
        assert!(!config.include_hot_paths);
        assert!(!config.include_call_stack_details);

        let config = ReportConfig::from_template(ReportTemplate::Detailed);
        assert!(config.include_thread_summary);
        assert!(config.include_hot_paths);
        assert!(config.include_call_stack_details);
    }

    #[test]
    fn test_report_section_title() {
        assert_eq!(ReportSection::Header.title(), "Report Header");
        assert_eq!(ReportSection::Summary.title(), "Summary");
        assert_eq!(ReportSection::FunctionStatsByThread.title(), "Function Statistics by Thread");
    }
}
