//! 报告格式化工具模块
//!
//! 提供报告格式化的辅助功能，包括函数名格式化、文件路径格式化、
//! 调用堆栈格式化、时间范围格式化以及CSV字段转义处理。

use crate::types::Timestamp;
use std::path::Path;

/// 报告格式化工具
///
/// 提供各种格式化辅助函数，用于处理报告中的字段格式。
///
/// # 使用示例
///
/// ```rust
/// use profiler::report::formatter::ReportFormatter;
/// use std::path::Path;
///
/// // 格式化函数名
/// let name = ReportFormatter::format_function_name("very_long_function_name_that_needs_truncation", 32);
///
/// // 格式化文件路径
/// let path = ReportFormatter::format_file_path("C:\\project\\src\\main.rs", Some(Path::new("C:\\project")));
///
/// // 格式化调用堆栈
/// let stack = vec!["func1".to_string(), "func2".to_string(), "func3".to_string()];
/// let formatted = ReportFormatter::format_call_stack(&stack, 10);
/// ```
pub struct ReportFormatter;

impl ReportFormatter {
    /// 格式化函数名
    ///
    /// 将函数名截断或格式化为指定长度，确保报告的可读性。
    ///
    /// # 参数
    /// - `name`: 原始函数名
    /// - `max_length`: 最大长度限制
    ///
    /// # 返回
    /// 格式化后的函数名字符串
    ///
    /// # 示例
    /// ```
    /// use profiler::report::formatter::ReportFormatter;
    ///
    /// let short = ReportFormatter::format_function_name("main", 32);
    /// assert_eq!(short, "main");
    ///
    /// let long = ReportFormatter::format_function_name(
    ///     "very_long_function_name_that_exceeds_limit",
    ///     20
    /// );
    /// assert!(long.len() <= 20);
    /// ```
    pub fn format_function_name(name: &str, max_length: usize) -> String {
        if name.len() <= max_length {
            name.to_string()
        } else {
            // 智能截断：保留函数名最后部分（通常更有意义）
            let separator = '!';
            if let Some(pos) = name.rfind(separator) {
                let module_part = &name[..pos];
                let func_part = &name[pos + 1..];
                
                let available = max_length.saturating_sub(3); // 为"..."预留空间
                if func_part.len() >= available {
                    // 如果函数名本身就很长，只截断函数名
                    format!("...{}", &func_part[func_part.len() - available..])
                } else {
                    // 保留模块的一部分
                    let module_keep = available.saturating_sub(func_part.len() + 1);
                    if module_keep > 0 && module_part.len() > module_keep {
                        format!("...{}!{}", 
                            &module_part[module_part.len() - module_keep..],
                            func_part
                        )
                    } else {
                        format!("...{}", func_part)
                    }
                }
            } else {
                // 没有模块分隔符，直接截断
                format!("...{}", &name[name.len() - max_length.saturating_sub(3)..])
            }
        }
    }

    /// 格式化文件路径
    ///
    /// 将文件路径转换为相对路径（如果指定了基础目录），或进行截断处理。
    ///
    /// # 参数
    /// - `path`: 原始文件路径
    /// - `base_dir`: 可选的基础目录，用于计算相对路径
    ///
    /// # 返回
    /// 格式化后的文件路径字符串
    pub fn format_file_path(path: &str, base_dir: Option<&Path>) -> String {
        let path_obj = Path::new(path);
        
        // 尝试转换为相对路径
        if let Some(base) = base_dir {
            if let Ok(relative) = path_obj.strip_prefix(base) {
                return relative.to_string_lossy().to_string();
            }
        }
        
        // 如果路径过长，进行智能截断
        const MAX_PATH_LENGTH: usize = 256;
        if path.len() > MAX_PATH_LENGTH {
            // 保留文件名部分
            if let Some(filename) = path_obj.file_name() {
                let filename_str = filename.to_string_lossy();
                let prefix_len = MAX_PATH_LENGTH.saturating_sub(filename_str.len() + 4); // 为".../"预留
                if prefix_len > 10 {
                    format!(".../{}", filename_str)
                } else {
                    filename_str.to_string()
                }
            } else {
                // 无法获取文件名，直接截断
                format!("...{}", &path[path.len() - MAX_PATH_LENGTH.saturating_sub(3)..])
            }
        } else {
            path.to_string()
        }
    }

    /// 格式化调用堆栈为单行字符串
    ///
    /// 将调用堆栈转换为分号分隔的单行字符串，便于CSV存储。
    ///
    /// # 参数
    /// - `stack`: 调用堆栈帧列表（从栈顶到栈底）
    /// - `max_depth`: 最大深度限制
    ///
    /// # 返回
    /// 格式化后的调用堆栈字符串
    ///
    /// # 示例
    /// ```
    /// use profiler::report::formatter::ReportFormatter;
    ///
    /// let stack = vec![
    ///     "main".to_string(),
    ///     "process_request".to_string(),
    ///     "handle_connection".to_string(),
    /// ];
    /// let formatted = ReportFormatter::format_call_stack(&stack, 10);
    /// assert_eq!(formatted, "main;process_request;handle_connection");
    /// ```
    pub fn format_call_stack(stack: &[String], max_depth: usize) -> String {
        let depth = stack.len().min(max_depth);
        let frames: Vec<String> = stack[..depth]
            .iter()
            .map(|f| Self::escape_csv_field_internal(f))
            .collect();
        
        frames.join(";")
    }

    /// 格式化时间范围
    ///
    /// 将起始和结束时间戳格式化为人类可读的时间范围字符串。
    ///
    /// # 参数
    /// - `start`: 起始时间戳（微秒）
    /// - `end`: 结束时间戳（微秒）
    ///
    /// # 返回
    /// 格式化后的时间范围字符串
    pub fn format_time_range(start: Timestamp, end: Timestamp) -> String {
        let duration = end.saturating_sub(start);
        format!(
            "{} - {} ({})",
            Self::format_timestamp(start),
            Self::format_timestamp(end),
            Self::format_duration(duration)
        )
    }

    /// 格式化时间戳
    ///
    /// 将Unix时间戳（微秒）格式化为人类可读的字符串。
    fn format_timestamp(timestamp: Timestamp) -> String {
        use chrono::TimeZone;
        
        // 将微秒转换为秒和纳秒
        let secs = (timestamp / 1_000_000) as i64;
        let nanos = ((timestamp % 1_000_000) * 1000) as u32;
        
        // 转换为本地时间
        if let Some(dt) = chrono::Local.timestamp_opt(secs, nanos).single() {
            dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
        } else {
            format!("{} μs", timestamp)
        }
    }

    /// 格式化时长
    ///
    /// 将微秒数格式化为人类可读的时长字符串。
    fn format_duration(microseconds: u64) -> String {
        if microseconds < 1000 {
            format!("{} μs", microseconds)
        } else if microseconds < 1_000_000 {
            format!("{:.2} ms", microseconds as f64 / 1000.0)
        } else if microseconds < 60_000_000 {
            format!("{:.2} s", microseconds as f64 / 1_000_000.0)
        } else {
            let seconds = microseconds / 1_000_000;
            let minutes = seconds / 60;
            let remaining_secs = seconds % 60;
            format!("{}m {}s", minutes, remaining_secs)
        }
    }

    /// 转义CSV字段（公开版本）
    ///
    /// 处理CSV字段中的特殊字符（逗号、引号、换行符）。
    ///
    /// # 参数
    /// - `field`: 原始字段值
    /// - `delimiter`: 分隔符字符
    ///
    /// # 返回
    /// 转义后的字段值
    pub fn escape_csv_field(field: &str, delimiter: char) -> String {
        let needs_quoting = field.contains(delimiter) 
            || field.contains('"') 
            || field.contains('\n') 
            || field.contains('\r');
        
        if needs_quoting {
            // 将所有双引号替换为两个双引号
            let escaped = field.replace('"', "\"\"");
            format!("\"{}\"", escaped)
        } else {
            field.to_string()
        }
    }

    /// 内部CSV字段转义（使用默认逗号分隔符）
    fn escape_csv_field_internal(field: &str) -> String {
        Self::escape_csv_field(field, ',')
    }

    /// 格式化百分比
    ///
    /// 将浮点数值格式化为百分比字符串。
    ///
    /// # 参数
    /// - `value`: 浮点数值（0.0 - 100.0）
    /// - `precision`: 小数精度
    ///
    /// # 返回
    /// 格式化后的百分比字符串
    pub fn format_percentage(value: f64, precision: usize) -> String {
        format!("{:.prec$}%", value, prec = precision)
    }

    /// 格式化数字（添加千位分隔符）
    ///
    /// # 参数
    /// - `value`: 整数值
    ///
    /// # 返回
    /// 格式化后的数字字符串
    pub fn format_number(value: u64) -> String {
        let mut result = String::new();
        let s = value.to_string();
        let len = s.len();
        
        for (i, ch) in s.chars().enumerate() {
            if i > 0 && (len - i) % 3 == 0 {
                result.push(',');
            }
            result.push(ch);
        }
        
        result
    }

    /// 格式化模块名
    ///
    /// 从完整路径中提取模块文件名。
    ///
    /// # 参数
    /// - `module_path`: 模块完整路径
    ///
    /// # 返回
    /// 模块文件名
    pub fn format_module_name(module_path: &str) -> String {
        Path::new(module_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| module_path.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_function_name_short() {
        let result = ReportFormatter::format_function_name("main", 32);
        assert_eq!(result, "main");
    }

    #[test]
    fn test_format_function_name_long() {
        let result = ReportFormatter::format_function_name(
            "very_long_function_name_that_exceeds_limit",
            20
        );
        assert!(result.len() <= 20);
        assert!(result.starts_with("..."));
    }

    #[test]
    fn test_format_function_name_with_module() {
        let result = ReportFormatter::format_function_name(
            "my_dll.dll!MyClass::VeryLongMethodName",
            25
        );
        assert!(result.len() <= 25);
        // 应该保留函数名部分
        assert!(result.contains("VeryLongMethodName") || result.contains("MyClass"));
    }

    #[test]
    fn test_format_file_path_relative() {
        let path = "C:\\project\\src\\main.rs";
        let base = Path::new("C:\\project");
        let result = ReportFormatter::format_file_path(path, Some(base));
        assert_eq!(result, "src\\main.rs");
    }

    #[test]
    fn test_format_file_path_absolute() {
        let path = "C:\\project\\src\\main.rs";
        let result = ReportFormatter::format_file_path(path, None);
        assert_eq!(result, "C:\\project\\src\\main.rs");
    }

    #[test]
    fn test_format_call_stack() {
        let stack = vec![
            "main".to_string(),
            "process_request".to_string(),
            "handle_connection".to_string(),
        ];
        let result = ReportFormatter::format_call_stack(&stack, 10);
        assert_eq!(result, "main;process_request;handle_connection");
    }

    #[test]
    fn test_format_call_stack_with_limit() {
        let stack = vec![
            "f1".to_string(),
            "f2".to_string(),
            "f3".to_string(),
            "f4".to_string(),
        ];
        let result = ReportFormatter::format_call_stack(&stack, 2);
        assert_eq!(result, "f1;f2");
    }

    #[test]
    fn test_escape_csv_field() {
        assert_eq!(
            ReportFormatter::escape_csv_field("simple", ','),
            "simple"
        );
        
        assert_eq!(
            ReportFormatter::escape_csv_field("with,comma", ','),
            "\"with,comma\""
        );
        
        assert_eq!(
            ReportFormatter::escape_csv_field("with\"quote", ','),
            "\"with\"\"quote\""
        );
        
        assert_eq!(
            ReportFormatter::escape_csv_field("with\nnewline", ','),
            "\"with\nnewline\""
        );
    }

    #[test]
    fn test_format_percentage() {
        assert_eq!(ReportFormatter::format_percentage(50.0, 2), "50.00%");
        assert_eq!(ReportFormatter::format_percentage(33.333, 1), "33.3%");
        assert_eq!(ReportFormatter::format_percentage(0.0, 0), "0%");
    }

    #[test]
    fn test_format_number() {
        assert_eq!(ReportFormatter::format_number(1000), "1,000");
        assert_eq!(ReportFormatter::format_number(1000000), "1,000,000");
        assert_eq!(ReportFormatter::format_number(999), "999");
    }

    #[test]
    fn test_format_module_name() {
        assert_eq!(
            ReportFormatter::format_module_name("C:\\Windows\\System32\\kernel32.dll"),
            "kernel32.dll"
        );
        assert_eq!(
            ReportFormatter::format_module_name("kernel32.dll"),
            "kernel32.dll"
        );
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(ReportFormatter::format_duration(500), "500 μs");
        assert_eq!(ReportFormatter::format_duration(1500), "1.50 ms");
        assert_eq!(ReportFormatter::format_duration(1_500_000), "1.50 s");
        assert_eq!(ReportFormatter::format_duration(65_000_000), "1m 5s");
    }
}
