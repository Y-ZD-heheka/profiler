//! CSV报告生成器模块
//!
//! 提供专门化的CSV性能报告生成功能，支持按线程分类的函数统计、
//! 热点调用路径、调用堆栈详情等多种报告表格。

use crate::analysis::{AnalysisResult, CallPath};
use crate::error::{ProfilerError, Result};
use crate::report::{ReportConfig, ReportGenerator, ReportSection};
use crate::report::formatter::ReportFormatter;
use crate::report::summary::ReportSummary;
use crate::types::{FunctionStats, ThreadId, ThreadStats};

use csv::Writer;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use tracing::{debug, info, warn};

/// CSV报告生成器
///
/// 专门用于生成CSV格式的性能分析报告，支持多种表格类型和详细配置。
#[derive(Debug, Clone)]
pub struct CsvReportGenerator {
    config: ReportConfig,
}

impl CsvReportGenerator {
    /// 创建新的CSV报告生成器
    pub fn new(config: ReportConfig) -> Self {
        Self { config }
    }

    /// 追加报告章节到CSV写入器
    pub fn append_section<W: Write>(
        &self,
        writer: &mut Writer<W>,
        section: ReportSection,
        result: &AnalysisResult,
    ) -> Result<()> {
        match section {
            ReportSection::Header => self.write_header(writer, result),
            ReportSection::Summary => self.write_summary(writer, result),
            ReportSection::FunctionStatsByThread => self.write_function_stats_by_thread(writer, result),
            ReportSection::ThreadSummary => self.write_thread_summary(writer, result),
            ReportSection::HotPaths => self.write_hot_paths(writer, result),
            ReportSection::CallStackDetails => {
                if self.config.include_call_stack_details {
                    self.write_call_stack_details(writer, result)
                } else {
                    Ok(())
                }
            }
            ReportSection::Footer => self.write_footer(writer, result),
        }
    }

    /// 生成所有章节
    fn generate_all_sections<W: Write>(
        &self,
        writer: &mut Writer<W>,
        result: &AnalysisResult,
    ) -> Result<()> {
        self.append_section(writer, ReportSection::Header, result)?;
        self.append_section(writer, ReportSection::Summary, result)?;
        self.append_section(writer, ReportSection::FunctionStatsByThread, result)?;
        
        if self.config.include_thread_summary {
            self.append_section(writer, ReportSection::ThreadSummary, result)?;
        }
        if self.config.include_hot_paths {
            self.append_section(writer, ReportSection::HotPaths, result)?;
        }
        if self.config.include_call_stack_details {
            self.append_section(writer, ReportSection::CallStackDetails, result)?;
        }
        self.append_section(writer, ReportSection::Footer, result)?;
        
        Ok(())
    }

    /// 写入报告头部
    fn write_header<W: Write>(
        &self,
        writer: &mut Writer<W>,
        result: &AnalysisResult,
    ) -> Result<()> {
        if self.config.include_bom {
            let bom: &[u8] = b"\xEF\xBB\xBF";
            writer.write_record(std::str::from_utf8(bom).map(|s| vec![s]).unwrap_or_default().as_slice())
                .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        }

        let now = chrono::Local::now();
        let timestamp = now.format(&self.config.time_format).to_string();

        writer.write_record(&["# ETW Performance Profiler Report"])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        writer.write_record(&[format!("# Generated: {}", timestamp)])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        writer.write_record(&[format!(
            "# Target Process: {} (PID: {})",
            result.process_stats.process_name.as_deref().unwrap_or("Unknown"),
            result.process_stats.process_id
        )])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        writer.write_record(&[format!("# Total Samples: {}", result.total_samples)])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        writer.write_record(&[format!("# Total Threads: {}", result.thread_stats.len())])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        writer.write_record(&[] as &[&str])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        Ok(())
    }

    /// 写入摘要信息
    fn write_summary<W: Write>(
        &self,
        writer: &mut Writer<W>,
        result: &AnalysisResult,
    ) -> Result<()> {
        let summary = ReportSummary::generate(result);

        writer.write_record(&[ReportSection::Summary.comment_marker()])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        writer.write_record(&["Metric", "Value"])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        writer.write_record(&["Analysis Duration (ms)", &summary.analysis_duration.as_millis().to_string()])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        writer.write_record(&["Total Samples", &summary.total_samples.to_string()])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        writer.write_record(&["Total Threads", &summary.total_threads.to_string()])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        writer.write_record(&["Total Functions", &summary.total_functions.to_string()])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        for (i, hotspot) in summary.top_hotspots.iter().enumerate().take(5) {
            writer.write_record(&[
                &format!("Top {} Function", i + 1),
                &format!("{} ({} ms, {:.1}%)", hotspot.function_name, hotspot.total_time_ms, hotspot.percentage),
            ])
                .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        }

        writer.write_record(&[] as &[&str])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        Ok(())
    }

    /// 写入函数统计（按线程分类）
    fn write_function_stats_by_thread<W: Write>(
        &self,
        writer: &mut Writer<W>,
        result: &AnalysisResult,
    ) -> Result<()> {
        writer.write_record(&[ReportSection::FunctionStatsByThread.comment_marker()])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        if self.config.include_headers {
            writer.write_record(&[
                "ThreadID", "Rank", "FunctionName", "ModuleName", "FilePath", "LineNumber",
                "TotalTime_ms", "SelfTime_ms", "TotalPercent", "SelfPercent", "SampleCount",
            ])
                .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        }

        let total_time: u64 = result.thread_stats.values().map(|t| t.total_execution_time_us).sum();
        let mut thread_ids: Vec<ThreadId> = result.thread_stats.keys().copied().collect();
        thread_ids.sort_unstable();

        for thread_id in thread_ids {
            if let Some(thread_stats) = result.thread_stats.get(&thread_id) {
                let mut functions: Vec<&FunctionStats> = thread_stats
                    .function_stats
                    .values()
                    .filter(|f| self.config.include_system_functions || !self.is_system_function(f))
                    .collect();

                functions.sort_by(|a, b| b.total_time_us.cmp(&a.total_time_us));

                for (rank, func) in functions.iter().enumerate() {
                    let thread_total = thread_stats.total_execution_time_us.max(1);
                    let total_percent = (func.total_time_us as f64 / total_time.max(1) as f64) * 100.0;
                    let self_percent = (func.self_time_us as f64 / thread_total as f64) * 100.0;

                    let module_name = func.module_name.clone().unwrap_or_else(|| "N/A".to_string());

                    writer.write_record(&[
                        &thread_id.to_string(),
                        &(rank + 1).to_string(),
                        &ReportFormatter::format_function_name(&func.function_name, self.config.max_function_name_length),
                        &module_name,
                        "N/A",
                        "N/A",
                        &format!("{:.prec$}", func.total_time_us as f64 / 1000.0, prec = self.config.float_precision),
                        &format!("{:.prec$}", func.self_time_us as f64 / 1000.0, prec = self.config.float_precision),
                        &format!("{:.prec$}%", total_percent, prec = self.config.float_precision),
                        &format!("{:.prec$}%", self_percent, prec = self.config.float_precision),
                        &func.call_count.to_string(),
                    ])
                        .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
                }
            }
        }

        writer.write_record(&[] as &[&str])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        Ok(())
    }

    /// 写入线程摘要
    fn write_thread_summary<W: Write>(
        &self,
        writer: &mut Writer<W>,
        result: &AnalysisResult,
    ) -> Result<()> {
        writer.write_record(&[ReportSection::ThreadSummary.comment_marker()])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        if self.config.include_headers {
            writer.write_record(&["ThreadID", "ThreadName", "TotalSamples", "TotalTime_ms", "CPU_Usage_Percent", "Top3Functions"])
                .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        }

        let total_system_samples: u64 = result.thread_stats.values().map(|t| t.total_samples).sum();
        let mut thread_ids: Vec<ThreadId> = result.thread_stats.keys().copied().collect();
        thread_ids.sort_unstable();

        for thread_id in thread_ids {
            if let Some(thread_stats) = result.thread_stats.get(&thread_id) {
                let top_functions = thread_stats.top_functions_by_time(3);
                let top3_names: Vec<String> = top_functions
                    .iter()
                    .map(|f| ReportFormatter::format_function_name(f.simple_name(), 32))
                    .collect();

                let cpu_usage = if total_system_samples > 0 {
                    (thread_stats.total_samples as f64 / total_system_samples as f64) * 100.0
                } else {
                    0.0
                };

                writer.write_record(&[
                    &thread_id.to_string(),
                    &format!("Thread {}", thread_id),
                    &thread_stats.total_samples.to_string(),
                    &format!("{:.prec$}", thread_stats.total_execution_time_us as f64 / 1000.0, prec = self.config.float_precision),
                    &format!("{:.prec$}", cpu_usage, prec = self.config.float_precision),
                    &top3_names.join(";"),
                ])
                    .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
            }
        }

        writer.write_record(&[] as &[&str])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        Ok(())
    }

    /// 写入热点调用路径
    fn write_hot_paths<W: Write>(
        &self,
        writer: &mut Writer<W>,
        result: &AnalysisResult,
    ) -> Result<()> {
        writer.write_record(&[ReportSection::HotPaths.comment_marker()])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        if self.config.include_headers {
            writer.write_record(&["ThreadID", "Rank", "PathDepth", "CallPath", "ExecutionCount", "EstimatedTime_ms"])
                .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        }

        // 简化处理：所有热点路径都归于线程0
        let paths: Vec<&CallPath> = result.hot_paths.iter().take(self.config.max_hot_paths).collect();
        
        for (rank, path) in paths.iter().enumerate() {
            let path_str = ReportFormatter::format_call_stack(&path.functions, self.config.max_stack_depth);

            writer.write_record(&[
                "0", // ThreadID - simplified
                &(rank + 1).to_string(),
                &path.depth.to_string(),
                &path_str,
                &path.count.to_string(),
                &format!("{:.prec$}", path.total_time_us as f64 / 1000.0, prec = self.config.float_precision),
            ])
                .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        }

        writer.write_record(&[] as &[&str])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        Ok(())
    }

    /// 写入调用堆栈详情
    fn write_call_stack_details<W: Write>(
        &self,
        writer: &mut Writer<W>,
        result: &AnalysisResult,
    ) -> Result<()> {
        writer.write_record(&[ReportSection::CallStackDetails.comment_marker()])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        if self.config.include_headers {
            writer.write_record(&["ThreadID", "FunctionName", "StackDepth", "CallerChain", "CallCount"])
                .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        }

        for (thread_id, thread_stats) in &result.thread_stats {
            let mut functions: Vec<&FunctionStats> = thread_stats.function_stats.values().collect();
            functions.sort_by(|a, b| b.call_count.cmp(&a.call_count));

            for func in functions.iter().take(self.config.max_hot_paths) {
                writer.write_record(&[
                    &thread_id.to_string(),
                    &ReportFormatter::format_function_name(&func.function_name, self.config.max_function_name_length),
                    "1",
                    &func.function_name,
                    &func.call_count.to_string(),
                ])
                    .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
            }
        }

        writer.write_record(&[] as &[&str])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        Ok(())
    }

    /// 写入报告尾部
    fn write_footer<W: Write>(
        &self,
        writer: &mut Writer<W>,
        _result: &AnalysisResult,
    ) -> Result<()> {
        writer.write_record(&["# End of Report"])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        Ok(())
    }

    /// 检查是否为系统函数
    fn is_system_function(&self, func: &FunctionStats) -> bool {
        let system_modules = [
            "ntdll.dll", "kernel32.dll", "kernelbase.dll", "user32.dll",
            "gdi32.dll", "advapi32.dll", "shell32.dll", "msvcrt.dll",
            "ucrtbase.dll", "ws2_32.dll",
        ];

        if let Some(ref module) = func.module_name {
            let module_lower = module.to_lowercase();
            system_modules.iter().any(|&sys| module_lower.contains(sys))
        } else {
            false
        }
    }
}

impl ReportGenerator for CsvReportGenerator {
    fn generate(&self, result: &AnalysisResult, output_path: &Path) -> Result<()> {
        info!("Generating CSV report to {:?}", output_path);

        let file = std::fs::File::create(output_path).map_err(|e| {
            ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to create report file: {}", e)))
        })?;

        let mut writer = Writer::from_writer(file);
        self.generate_all_sections(&mut writer, result)?;

        writer.flush().map_err(|e| {
            ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to flush report file: {}", e)))
        })?;

        info!("CSV report generated successfully: {:?}", output_path);
        Ok(())
    }

    fn generate_to_string(&self, result: &AnalysisResult) -> Result<String> {
        let mut buffer = Vec::new();
        {
            let mut writer = Writer::from_writer(&mut buffer);
            self.generate_all_sections(&mut writer, result)?;
            writer.flush().map_err(|e| {
                ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to flush report buffer: {}", e)))
            })?;
        }

        String::from_utf8(buffer).map_err(|e| {
            ProfilerError::Generic(format!("Invalid UTF-8 in report: {}", e))
        })
    }

    fn config(&self) -> &ReportConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ProcessStats, FunctionStats};

    fn create_test_analysis_result() -> AnalysisResult {
        let process_stats = ProcessStats::new(1234);
        let mut result = AnalysisResult::new(process_stats);
        
        let mut thread_stats = ThreadStats::new(1, 1234);
        thread_stats.total_samples = 100;
        thread_stats.total_execution_time_us = 100000;
        
        let mut func_stats = FunctionStats::new("test.dll!TestFunction");
        func_stats.total_time_us = 50000;
        func_stats.self_time_us = 10000;
        func_stats.call_count = 50;
        thread_stats.add_function_stats(func_stats);
        
        result.add_thread_stats(thread_stats);
        result.total_samples = 100;
        result.analysis_start_time = 0;
        result.analysis_end_time = 1000000;
        
        result
    }

    #[test]
    fn test_csv_report_generator_new() {
        let config = ReportConfig::default();
        let generator = CsvReportGenerator::new(config);
        assert_eq!(generator.config().title, "ETW Performance Profiler Report");
    }

    #[test]
    fn test_csv_report_generator_to_string() {
        let config = ReportConfig::default();
        let generator = CsvReportGenerator::new(config);
        let result = create_test_analysis_result();
        
        let csv_string = generator.generate_to_string(&result).unwrap();
        assert!(!csv_string.is_empty());
        assert!(csv_string.contains("ETW Performance Profiler Report"));
        assert!(csv_string.contains("TestFunction"));
    }

    #[test]
    fn test_is_system_function() {
        let config = ReportConfig::default();
        let generator = CsvReportGenerator::new(config);
        
        let mut func = FunctionStats::new("ntdll.dll!RtlInitializeExceptionChain");
        func.module_name = Some("ntdll.dll".to_string());
        assert!(generator.is_system_function(&func));
        
        let mut func2 = FunctionStats::new("myapp.exe!main");
        func2.module_name = Some("myapp.exe".to_string());
        assert!(!generator.is_system_function(&func2));
    }
}
