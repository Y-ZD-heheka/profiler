//! 报告模板模块
//!
//! 提供预定义的报告格式模板，支持详细报告、摘要报告、
//! 线程聚焦报告和函数聚焦报告等不同类型。

use crate::report::ReportConfig;

/// 报告模板枚举
///
/// 定义不同类型的报告模板，每种模板都有预设的配置选项。
///
/// # 模板类型
///
/// - `Detailed`: 详细报告 - 包含所有表格和详细信息
/// - `Summary`: 摘要报告 - 仅包含关键统计信息
/// - `ThreadFocused`: 线程聚焦报告 - 重点关注线程统计
/// - `FunctionFocused`: 函数聚焦报告 - 重点关注函数统计和调用堆栈
///
/// # 使用示例
///
/// ```rust
/// use profiler::report::templates::{ReportTemplate, TemplateConfig};
///
/// // 创建详细报告配置
/// let config = ReportTemplate::Detailed.config();
///
/// // 从字符串解析模板类型
/// let template = ReportTemplate::parse("summary");
/// assert!(template.is_some());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReportTemplate {
    /// 详细报告
    ///
    /// 包含所有表格：函数统计、线程摘要、热点路径、调用堆栈详情
    #[default]
    Detailed,
    /// 摘要报告
    ///
    /// 仅包含关键统计信息和前10个热点函数
    Summary,
    /// 线程聚焦报告
    ///
    /// 重点关注线程统计和线程间的比较
    ThreadFocused,
    /// 函数聚焦报告
    ///
    /// 重点关注函数统计和调用关系
    FunctionFocused,
}

impl ReportTemplate {
    /// 获取模板的配置
    ///
    /// # 返回
    /// 预设的报告配置
    pub fn config(&self) -> ReportConfig {
        match self {
            ReportTemplate::Detailed => Self::detailed_config(),
            ReportTemplate::Summary => Self::summary_config(),
            ReportTemplate::ThreadFocused => Self::thread_focused_config(),
            ReportTemplate::FunctionFocused => Self::function_focused_config(),
        }
    }

    /// 获取模板名称
    ///
    /// # 返回
    /// 模板的字符串名称
    pub fn name(&self) -> &'static str {
        match self {
            ReportTemplate::Detailed => "detailed",
            ReportTemplate::Summary => "summary",
            ReportTemplate::ThreadFocused => "thread-focused",
            ReportTemplate::FunctionFocused => "function-focused",
        }
    }

    /// 获取模板显示名称
    ///
    /// # 返回
    /// 适合用户界面显示的模板名称
    pub fn display_name(&self) -> &'static str {
        match self {
            ReportTemplate::Detailed => "Detailed Report",
            ReportTemplate::Summary => "Summary Report",
            ReportTemplate::ThreadFocused => "Thread-Focused Report",
            ReportTemplate::FunctionFocused => "Function-Focused Report",
        }
    }

    /// 获取模板描述
    ///
    /// # 返回
    /// 模板的详细描述
    pub fn description(&self) -> &'static str {
        match self {
            ReportTemplate::Detailed => {
                "Complete performance report with all tables and details: \
                 function statistics by thread, thread summary, hot call paths, \
                 and call stack details."
            }
            ReportTemplate::Summary => {
                "Brief report containing only key statistics and top 10 hotspots. \
                 Suitable for quick overview."
            }
            ReportTemplate::ThreadFocused => {
                "Thread-centric report focusing on thread statistics, \
                 CPU usage per thread, and thread-level analysis."
            }
            ReportTemplate::FunctionFocused => {
                "Function-centric report emphasizing function statistics, \
                 call relationships, and hot paths."
            }
        }
    }

    /// 获取包含的章节列表
    ///
    /// # 返回
    /// 该模板包含的章节名称列表
    pub fn included_sections(&self) -> Vec<&'static str> {
        match self {
            ReportTemplate::Detailed => vec![
                "Header",
                "Summary",
                "Function Statistics by Thread",
                "Thread Summary",
                "Hot Call Paths",
                "Call Stack Details",
                "Footer",
            ],
            ReportTemplate::Summary => vec![
                "Header",
                "Summary",
                "Top Hotspots",
                "Thread Summary",
                "Footer",
            ],
            ReportTemplate::ThreadFocused => vec![
                "Header",
                "Summary",
                "Thread Summary",
                "Function Statistics by Thread",
                "Hot Call Paths",
                "Footer",
            ],
            ReportTemplate::FunctionFocused => vec![
                "Header",
                "Summary",
                "Function Statistics by Thread",
                "Hot Call Paths",
                "Call Stack Details",
                "Footer",
            ],
        }
    }

    /// 检查是否包含特定章节
    ///
    /// # 参数
    /// - `section`: 章节名称
    ///
    /// # 返回
    /// 如果包含该章节返回true，否则返回false
    pub fn has_section(&self, section: &str) -> bool {
        self.included_sections().contains(&section)
    }

    /// 从字符串解析模板类型
    ///
    /// # 参数
    /// - `s`: 模板名称字符串
    ///
    /// # 返回
    /// 解析成功返回Some(ReportTemplate)，失败返回None
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "detailed" | "full" | "complete" => Some(ReportTemplate::Detailed),
            "summary" | "brief" | "overview" => Some(ReportTemplate::Summary),
            "thread" | "thread-focused" | "threadfocus" => Some(ReportTemplate::ThreadFocused),
            "function" | "function-focused" | "functionfocus" => {
                Some(ReportTemplate::FunctionFocused)
            }
            _ => None,
        }
    }

    /// 获取所有可用模板
    ///
    /// # 返回
    /// 所有模板类型的列表
    pub fn all() -> Vec<ReportTemplate> {
        vec![
            ReportTemplate::Detailed,
            ReportTemplate::Summary,
            ReportTemplate::ThreadFocused,
            ReportTemplate::FunctionFocused,
        ]
    }

    // 私有辅助方法：详细报告配置
    fn detailed_config() -> ReportConfig {
        ReportConfig {
            title: "ETW Detailed Performance Report".to_string(),
            include_bom: true,
            delimiter: ',',
            include_headers: true,
            max_function_name_length: 128,
            max_file_path_length: 256,
            max_stack_depth: 32,
            max_hot_paths: 100,
            include_system_functions: true,
            include_thread_summary: true,
            include_hot_paths: true,
            include_call_stack_details: true,
            time_format: "%Y-%m-%d %H:%M:%S".to_string(),
            float_precision: 2,
            base_directory: None,
            template: ReportTemplate::Detailed,
        }
    }

    // 私有辅助方法：摘要报告配置
    fn summary_config() -> ReportConfig {
        ReportConfig {
            title: "ETW Performance Summary".to_string(),
            include_bom: true,
            delimiter: ',',
            include_headers: true,
            max_function_name_length: 64,
            max_file_path_length: 128,
            max_stack_depth: 10,
            max_hot_paths: 10,
            include_system_functions: false,
            include_thread_summary: true,
            include_hot_paths: false,
            include_call_stack_details: false,
            time_format: "%Y-%m-%d %H:%M".to_string(),
            float_precision: 1,
            base_directory: None,
            template: ReportTemplate::Summary,
        }
    }

    // 私有辅助方法：线程聚焦报告配置
    fn thread_focused_config() -> ReportConfig {
        ReportConfig {
            title: "ETW Thread Analysis Report".to_string(),
            include_bom: true,
            delimiter: ',',
            include_headers: true,
            max_function_name_length: 96,
            max_file_path_length: 200,
            max_stack_depth: 20,
            max_hot_paths: 50,
            include_system_functions: false,
            include_thread_summary: true,
            include_hot_paths: true,
            include_call_stack_details: false,
            time_format: "%Y-%m-%d %H:%M:%S".to_string(),
            float_precision: 2,
            base_directory: None,
            template: ReportTemplate::ThreadFocused,
        }
    }

    // 私有辅助方法：函数聚焦报告配置
    fn function_focused_config() -> ReportConfig {
        ReportConfig {
            title: "ETW Function Analysis Report".to_string(),
            include_bom: true,
            delimiter: ',',
            include_headers: true,
            max_function_name_length: 160,
            max_file_path_length: 300,
            max_stack_depth: 64,
            max_hot_paths: 100,
            include_system_functions: false,
            include_thread_summary: false,
            include_hot_paths: true,
            include_call_stack_details: true,
            time_format: "%Y-%m-%d %H:%M:%S".to_string(),
            float_precision: 3,
            base_directory: None,
            template: ReportTemplate::FunctionFocused,
        }
    }
}

impl std::fmt::Display for ReportTemplate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// 模板配置结构体
///
/// 用于管理多个模板配置和提供模板相关的工具方法。
#[derive(Debug, Clone)]
pub struct TemplateConfig {
    /// 当前选中的模板
    pub current_template: ReportTemplate,
    /// 自定义模板配置覆盖
    pub custom_overrides: Option<ReportConfig>,
}

impl TemplateConfig {
    /// 创建新的模板配置
    ///
    /// # 参数
    /// - `template`: 基础模板类型
    pub fn new(template: ReportTemplate) -> Self {
        Self {
            current_template: template,
            custom_overrides: None,
        }
    }

    /// 使用详细模板创建配置
    pub fn detailed() -> Self {
        Self::new(ReportTemplate::Detailed)
    }

    /// 使用摘要模板创建配置
    pub fn summary() -> Self {
        Self::new(ReportTemplate::Summary)
    }

    /// 使用线程聚焦模板创建配置
    pub fn thread_focused() -> Self {
        Self::new(ReportTemplate::ThreadFocused)
    }

    /// 使用函数聚焦模板创建配置
    pub fn function_focused() -> Self {
        Self::new(ReportTemplate::FunctionFocused)
    }

    /// 添加自定义配置覆盖
    ///
    /// # 参数
    /// - `config`: 自定义配置
    pub fn with_overrides(mut self, config: ReportConfig) -> Self {
        self.custom_overrides = Some(config);
        self
    }

    /// 获取最终配置
    ///
    /// 如果有自定义覆盖，则合并到基础模板配置中
    pub fn final_config(&self) -> ReportConfig {
        let mut config = self.current_template.config();
        
        if let Some(ref overrides) = self.custom_overrides {
            // 应用自定义覆盖
            if !overrides.title.is_empty() {
                config.title = overrides.title.clone();
            }
            config.include_bom = overrides.include_bom;
            config.delimiter = overrides.delimiter;
            config.include_headers = overrides.include_headers;
            config.max_function_name_length = overrides.max_function_name_length;
            config.max_file_path_length = overrides.max_file_path_length;
            config.max_stack_depth = overrides.max_stack_depth;
            config.max_hot_paths = overrides.max_hot_paths;
            config.include_system_functions = overrides.include_system_functions;
            config.include_thread_summary = overrides.include_thread_summary;
            config.include_hot_paths = overrides.include_hot_paths;
            config.include_call_stack_details = overrides.include_call_stack_details;
            if !overrides.time_format.is_empty() {
                config.time_format = overrides.time_format.clone();
            }
            config.float_precision = overrides.float_precision;
            if overrides.base_directory.is_some() {
                config.base_directory = overrides.base_directory.clone();
            }
        }
        
        config
    }

    /// 切换模板
    ///
    /// # 参数
    /// - `template`: 新模板类型
    pub fn switch_template(&mut self, template: ReportTemplate) {
        self.current_template = template;
    }

    /// 获取当前模板
    pub fn template(&self) -> ReportTemplate {
        self.current_template
    }

    /// 列出所有可用模板及其描述
    pub fn list_templates() -> Vec<(ReportTemplate, &'static str, &'static str)> {
        ReportTemplate::all()
            .into_iter()
            .map(|t| (t, t.display_name(), t.description()))
            .collect()
    }
}

impl Default for TemplateConfig {
    fn default() -> Self {
        Self {
            current_template: ReportTemplate::Detailed,
            custom_overrides: None,
        }
    }
}

/// 模板预设常量
///
/// 提供方便的常量访问常用模板配置。
pub mod presets {
    use super::*;

    /// 详细报告预设
    pub fn detailed() -> ReportConfig {
        ReportTemplate::Detailed.config()
    }

    /// 摘要报告预设
    pub fn summary() -> ReportConfig {
        ReportTemplate::Summary.config()
    }

    /// 线程聚焦报告预设
    pub fn thread_focused() -> ReportConfig {
        ReportTemplate::ThreadFocused.config()
    }

    /// 函数聚焦报告预设
    pub fn function_focused() -> ReportConfig {
        ReportTemplate::FunctionFocused.config()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_report_template_config() {
        let detailed = ReportTemplate::Detailed.config();
        assert!(detailed.include_call_stack_details);
        assert!(detailed.include_hot_paths);
        assert_eq!(detailed.max_hot_paths, 100);

        let summary = ReportTemplate::Summary.config();
        assert!(!summary.include_call_stack_details);
        assert!(!summary.include_hot_paths);
        assert_eq!(summary.max_hot_paths, 10);
    }

    #[test]
    fn test_report_template_parse() {
        assert_eq!(
            ReportTemplate::parse("detailed"),
            Some(ReportTemplate::Detailed)
        );
        assert_eq!(
            ReportTemplate::parse("SUMMARY"),
            Some(ReportTemplate::Summary)
        );
        assert_eq!(
            ReportTemplate::parse("thread-focused"),
            Some(ReportTemplate::ThreadFocused)
        );
        assert_eq!(
            ReportTemplate::parse("function"),
            Some(ReportTemplate::FunctionFocused)
        );
        assert_eq!(ReportTemplate::parse("unknown"), None);
    }

    #[test]
    fn test_report_template_name() {
        assert_eq!(ReportTemplate::Detailed.name(), "detailed");
        assert_eq!(ReportTemplate::Summary.name(), "summary");
    }

    #[test]
    fn test_report_template_sections() {
        let detailed_sections = ReportTemplate::Detailed.included_sections();
        assert!(detailed_sections.contains(&"Call Stack Details"));
        
        let summary_sections = ReportTemplate::Summary.included_sections();
        assert!(!summary_sections.contains(&"Call Stack Details"));
        assert!(summary_sections.contains(&"Top Hotspots"));
    }

    #[test]
    fn test_report_template_has_section() {
        assert!(ReportTemplate::Detailed.has_section("Call Stack Details"));
        assert!(!ReportTemplate::Summary.has_section("Call Stack Details"));
    }

    #[test]
    fn test_template_config() {
        let config = TemplateConfig::detailed();
        assert_eq!(config.template(), ReportTemplate::Detailed);
        
        let final_config = config.final_config();
        assert!(final_config.include_call_stack_details);
    }

    #[test]
    fn test_template_config_with_overrides() {
        let custom = ReportConfig::default()
            .with_title("Custom Title")
            .with_max_hot_paths(50);
        
        let config = TemplateConfig::summary()
            .with_overrides(custom);
        
        let final_config = config.final_config();
        assert_eq!(final_config.title, "Custom Title");
        assert_eq!(final_config.max_hot_paths, 50);
    }

    #[test]
    fn test_template_presets() {
        let detailed = presets::detailed();
        assert!(detailed.include_call_stack_details);
        
        let summary = presets::summary();
        assert!(!summary.include_call_stack_details);
    }

    #[test]
    fn test_template_display() {
        assert_eq!(
            format!("{}", ReportTemplate::Detailed),
            "Detailed Report"
        );
    }

    #[test]
    fn test_all_templates() {
        let all = ReportTemplate::all();
        assert_eq!(all.len(), 4);
        assert!(all.contains(&ReportTemplate::Detailed));
        assert!(all.contains(&ReportTemplate::Summary));
        assert!(all.contains(&ReportTemplate::ThreadFocused));
        assert!(all.contains(&ReportTemplate::FunctionFocused));
    }

    #[test]
    fn test_list_templates() {
        let templates = TemplateConfig::list_templates();
        assert_eq!(templates.len(), 4);
        
        // 检查是否包含预期的模板
        let names: Vec<&str> = templates.iter().map(|(_, name, _)| *name).collect();
        assert!(names.contains(&"Detailed Report"));
        assert!(names.contains(&"Summary Report"));
    }
}
