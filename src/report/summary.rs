//! 报告摘要生成模块
//!
//! 提供报告摘要信息的生成和管理功能，包括分析持续时间、
//! 总样本数、线程数、顶级热点等关键统计信息。

use crate::analysis::AnalysisResult;
use crate::error::{ProfilerError, Result};
use crate::types::FunctionStats;
use csv::Writer;
use std::io::Write;
use std::time::Duration;

/// 报告摘要结构体
///
/// 包含报告的关键统计信息和顶级热点。
#[derive(Debug, Clone)]
pub struct ReportSummary {
    /// 分析持续时间
    pub analysis_duration: Duration,
    /// 总样本数
    pub total_samples: u64,
    /// 线程数
    pub total_threads: usize,
    /// 总函数数
    pub total_functions: usize,
    /// 总进程数
    pub total_processes: usize,
    /// 顶级热点列表
    pub top_hotspots: Vec<HotspotSummary>,
}

impl ReportSummary {
    /// 创建新的报告摘要
    pub fn new(
        analysis_duration: Duration,
        total_samples: u64,
        total_threads: usize,
        top_hotspots: Vec<HotspotSummary>,
    ) -> Self {
        Self {
            analysis_duration,
            total_samples,
            total_threads,
            total_functions: 0,
            total_processes: 1,
            top_hotspots,
        }
    }

    /// 从分析结果生成摘要
    pub fn generate(result: &AnalysisResult) -> Self {
        let analysis_duration = Duration::from_micros(result.analysis_duration_us());
        let total_samples = result.total_samples;
        let total_threads = result.thread_stats.len();
        
        let total_functions: usize = result
            .thread_stats
            .values()
            .map(|t| t.function_stats.len())
            .sum();
        
        let total_time = result.total_sampling_time_ms() * 1000;
        
        let mut all_functions: Vec<&FunctionStats> = result
            .thread_stats
            .values()
            .flat_map(|t| t.function_stats.values())
            .collect();
        
        all_functions.sort_by(|a, b| b.total_time_us.cmp(&a.total_time_us));
        
        let top_hotspots: Vec<HotspotSummary> = all_functions
            .into_iter()
            .take(10)
            .map(|f| HotspotSummary::from_function_stats(f, total_time))
            .collect();
        
        Self {
            analysis_duration,
            total_samples,
            total_threads,
            total_functions,
            total_processes: 1,
            top_hotspots,
        }
    }

    /// 写入摘要头部到CSV写入器
    pub fn write_summary_header<W: Write>(&self, writer: &mut Writer<W>) -> Result<()> {
        writer.write_record(&["# Report Summary"])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        writer.write_record(&["Metric", "Value"])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        
        writer.write_record(&[
            "Analysis Duration (ms)",
            &self.analysis_duration.as_millis().to_string(),
        ])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        writer.write_record(&["Total Samples", &self.total_samples.to_string()])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        writer.write_record(&["Total Threads", &self.total_threads.to_string()])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        writer.write_record(&["Total Functions", &self.total_functions.to_string()])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        writer.write_record(&["Total Processes", &self.total_processes.to_string()])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        
        let samples_per_second = if self.analysis_duration.as_secs() > 0 {
            self.total_samples / self.analysis_duration.as_secs()
        } else {
            0
        };
        writer.write_record(&["Samples per Second", &samples_per_second.to_string()])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        
        writer.write_record(&[] as &[&str])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        
        if !self.top_hotspots.is_empty() {
            writer.write_record(&["# Top Hotspots"])
                .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
            writer.write_record(&[
                "Rank",
                "Function Name",
                "Total Time (ms)",
                "Self Time (ms)",
                "Call Count",
                "Percentage",
            ])
                .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
            
            for (i, hotspot) in self.top_hotspots.iter().enumerate() {
                writer.write_record(&[
                    &(i + 1).to_string(),
                    &hotspot.function_name,
                    &format!("{:.2}", hotspot.total_time_ms),
                    &format!("{:.2}", hotspot.self_time_ms),
                    &hotspot.call_count.to_string(),
                    &format!("{:.2}%", hotspot.percentage),
                ])
                    .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
            }
        }
        
        writer.write_record(&[] as &[&str])
            .map_err(|e| ProfilerError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        Ok(())
    }

    /// 获取分析持续时间（毫秒）
    pub fn analysis_duration_ms(&self) -> u64 {
        self.analysis_duration.as_millis() as u64
    }

    /// 获取分析持续时间（秒）
    pub fn analysis_duration_secs(&self) -> u64 {
        self.analysis_duration.as_secs()
    }

    /// 获取采样率（每秒样本数）
    pub fn sample_rate(&self) -> f64 {
        if self.analysis_duration.as_secs() > 0 {
            self.total_samples as f64 / self.analysis_duration.as_secs_f64()
        } else {
            0.0
        }
    }

    /// 转换为JSON格式的字符串
    pub fn to_json(&self) -> String {
        let hotspots_json: Vec<String> = self
            .top_hotspots
            .iter()
            .enumerate()
            .map(|(i, h)| {
                format!(
                    r#"{{"rank":{},"function_name":"{}","total_time_ms":{:.2},"percentage":{:.2}}}"#,
                    i + 1,
                    h.function_name.replace('"', "\\\""),
                    h.total_time_ms,
                    h.percentage
                )
            })
            .collect();
        
        format!(
            r#"{{"analysis_duration_ms":{},"total_samples":{},"total_threads":{},"total_functions":{},"top_hotspots":[{}]}}"#,
            self.analysis_duration_ms(),
            self.total_samples,
            self.total_threads,
            self.total_functions,
            hotspots_json.join(",")
        )
    }

    /// 转换为Markdown格式
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        
        md.push_str("## Report Summary\n\n");
        md.push_str("| Metric | Value |\n");
        md.push_str("|--------|-------|\n");
        md.push_str(&format!(
            "| Analysis Duration | {} ms |\n",
            self.analysis_duration_ms()
        ));
        md.push_str(&format!("| Total Samples | {} |\n", self.total_samples));
        md.push_str(&format!("| Total Threads | {} |\n", self.total_threads));
        md.push_str(&format!("| Total Functions | {} |\n", self.total_functions));
        
        if !self.top_hotspots.is_empty() {
            md.push_str("\n### Top Hotspots\n\n");
            md.push_str("| Rank | Function | Total Time (ms) | Percentage |\n");
            md.push_str("|------|----------|-----------------|------------|\n");
            
            for (i, hotspot) in self.top_hotspots.iter().enumerate() {
                md.push_str(&format!(
                    "| {} | {} | {:.2} | {:.2}% |\n",
                    i + 1,
                    hotspot.function_name,
                    hotspot.total_time_ms,
                    hotspot.percentage
                ));
            }
        }
        
        md
    }

    /// 合并另一个摘要（用于增量分析）
    pub fn merge(&mut self, other: &ReportSummary) {
        self.total_samples += other.total_samples;
        self.total_threads = self.total_threads.max(other.total_threads);
        self.total_functions += other.total_functions;
        self.total_processes = self.total_processes.max(other.total_processes);
        
        let mut merged_hotspots = self.top_hotspots.clone();
        merged_hotspots.extend_from_slice(&other.top_hotspots);
        
        merged_hotspots.sort_by(|a, b| b.percentage.partial_cmp(&a.percentage).unwrap());
        merged_hotspots.dedup_by(|a, b| a.function_name == b.function_name);
        
        self.top_hotspots = merged_hotspots.into_iter().take(10).collect();
        
        self.analysis_duration += other.analysis_duration;
    }
}

impl Default for ReportSummary {
    fn default() -> Self {
        Self {
            analysis_duration: Duration::default(),
            total_samples: 0,
            total_threads: 0,
            total_functions: 0,
            total_processes: 0,
            top_hotspots: Vec::new(),
        }
    }
}

impl std::fmt::Display for ReportSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== Performance Analysis Summary ===")?;
        writeln!(f, "Analysis Duration: {} ms", self.analysis_duration_ms())?;
        writeln!(f, "Total Samples: {}", self.total_samples)?;
        writeln!(f, "Total Threads: {}", self.total_threads)?;
        writeln!(f, "Total Functions: {}", self.total_functions)?;
        writeln!(f, "Sample Rate: {:.2} samples/sec", self.sample_rate())?;
        
        if !self.top_hotspots.is_empty() {
            writeln!(f, "\nTop 5 Hotspots:")?;
            for (i, hotspot) in self.top_hotspots.iter().take(5).enumerate() {
                writeln!(
                    f,
                    "  {}. {} - {:.2} ms ({:.2}%)",
                    i + 1,
                    hotspot.function_name,
                    hotspot.total_time_ms,
                    hotspot.percentage
                )?;
            }
        }
        
        Ok(())
    }
}

/// 热点摘要信息
///
/// 表示单个函数的热点信息。
#[derive(Debug, Clone)]
pub struct HotspotSummary {
    /// 函数完整名称
    pub function_name: String,
    /// 模块名称
    pub module_name: Option<String>,
    /// 总耗时（毫秒）
    pub total_time_ms: f64,
    /// 自身耗时（毫秒）
    pub self_time_ms: f64,
    /// 调用次数
    pub call_count: u64,
    /// 占总时间的百分比
    pub percentage: f64,
    /// 所属线程ID
    pub thread_id: Option<u32>,
}

impl HotspotSummary {
    /// 从函数统计创建热点摘要
    pub fn from_function_stats(stats: &FunctionStats, total_time: u64) -> Self {
        let percentage = if total_time > 0 {
            (stats.total_time_us as f64 / total_time as f64) * 100.0
        } else {
            0.0
        };
        
        Self {
            function_name: stats.function_name.clone(),
            module_name: stats.module_name.clone(),
            total_time_ms: stats.total_time_us as f64 / 1000.0,
            self_time_ms: stats.self_time_us as f64 / 1000.0,
            call_count: stats.call_count,
            percentage,
            thread_id: stats.thread_id,
        }
    }

    /// 创建新的热点摘要
    pub fn new(
        function_name: impl Into<String>,
        total_time_ms: f64,
        percentage: f64,
    ) -> Self {
        Self {
            function_name: function_name.into(),
            module_name: None,
            total_time_ms,
            self_time_ms: 0.0,
            call_count: 0,
            percentage,
            thread_id: None,
        }
    }

    /// 设置模块名称
    pub fn with_module(mut self, module: impl Into<String>) -> Self {
        self.module_name = Some(module.into());
        self
    }

    /// 设置自身耗时
    pub fn with_self_time(mut self, self_time_ms: f64) -> Self {
        self.self_time_ms = self_time_ms;
        self
    }

    /// 设置调用次数
    pub fn with_call_count(mut self, count: u64) -> Self {
        self.call_count = count;
        self
    }

    /// 设置线程ID
    pub fn with_thread_id(mut self, thread_id: u32) -> Self {
        self.thread_id = Some(thread_id);
        self
    }

    /// 获取简单函数名（不含模块）
    pub fn simple_name(&self) -> &str {
        match self.function_name.rfind('!') {
            Some(pos) => &self.function_name[pos + 1..],
            None => &self.function_name,
        }
    }

    /// 获取模块名（如果有）
    pub fn module(&self) -> &str {
        self.module_name
            .as_deref()
            .or_else(|| self.function_name.split('!').next())
            .unwrap_or("Unknown")
    }
}

impl Default for HotspotSummary {
    fn default() -> Self {
        Self {
            function_name: String::new(),
            module_name: None,
            total_time_ms: 0.0,
            self_time_ms: 0.0,
            call_count: 0,
            percentage: 0.0,
            thread_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionStats, ProcessStats, ThreadStats};
    use crate::analysis::AnalysisResult;

    fn create_test_analysis_result() -> AnalysisResult {
        let process_stats = ProcessStats::new(1234);
        let mut result = AnalysisResult::new(process_stats);
        
        let mut thread_stats = ThreadStats::new(1, 1234);
        thread_stats.total_samples = 100;
        
        let mut func1 = FunctionStats::new("test.dll!hot_function");
        func1.total_time_us = 50000;
        func1.self_time_us = 20000;
        func1.call_count = 50;
        thread_stats.add_function_stats(func1);
        
        let mut func2 = FunctionStats::new("test.dll!cold_function");
        func2.total_time_us = 10000;
        func2.self_time_us = 5000;
        func2.call_count = 10;
        thread_stats.add_function_stats(func2);
        
        result.add_thread_stats(thread_stats);
        result.total_samples = 100;
        result.analysis_start_time = 0;
        result.analysis_end_time = 1_000_000;
        
        result
    }

    #[test]
    fn test_report_summary_generate() {
        let result = create_test_analysis_result();
        let summary = ReportSummary::generate(&result);
        
        assert_eq!(summary.total_samples, 100);
        assert_eq!(summary.total_threads, 1);
        assert_eq!(summary.total_functions, 2);
        assert!(!summary.top_hotspots.is_empty());
    }

    #[test]
    fn test_hotspot_summary_from_function_stats() {
        let func = FunctionStats::new("test.dll!TestFunction");
        let hotspot = HotspotSummary::from_function_stats(&func, 1000000);
        
        assert_eq!(hotspot.function_name, "test.dll!TestFunction");
        assert!(hotspot.percentage >= 0.0);
    }

    #[test]
    fn test_hotspot_simple_name() {
        let hotspot = HotspotSummary::new("module!function_name", 100.0, 50.0);
        assert_eq!(hotspot.simple_name(), "function_name");
        
        let hotspot2 = HotspotSummary::new("function_name", 100.0, 50.0);
        assert_eq!(hotspot2.simple_name(), "function_name");
    }

    #[test]
    fn test_report_summary_to_markdown() {
        let result = create_test_analysis_result();
        let summary = ReportSummary::generate(&result);
        let md = summary.to_markdown();
        
        assert!(md.contains("Report Summary"));
        assert!(md.contains("Total Samples"));
        assert!(md.contains("hot_function"));
    }

    #[test]
    fn test_report_summary_to_json() {
        let result = create_test_analysis_result();
        let summary = ReportSummary::generate(&result);
        let json = summary.to_json();
        
        assert!(json.contains("analysis_duration_ms"));
        assert!(json.contains("total_samples"));
        assert!(json.contains("top_hotspots"));
    }

    #[test]
    fn test_report_summary_display() {
        let result = create_test_analysis_result();
        let summary = ReportSummary::generate(&result);
        let display = format!("{}", summary);
        
        assert!(display.contains("Performance Analysis Summary"));
        assert!(display.contains("Total Samples"));
    }

    #[test]
    fn test_report_summary_merge() {
        let mut summary1 = ReportSummary {
            total_samples: 100,
            total_threads: 2,
            total_functions: 10,
            ..Default::default()
        };
        
        let summary2 = ReportSummary {
            total_samples: 50,
            total_threads: 3,
            total_functions: 5,
            ..Default::default()
        };
        
        summary1.merge(&summary2);
        
        assert_eq!(summary1.total_samples, 150);
        assert_eq!(summary1.total_threads, 3);
        assert_eq!(summary1.total_functions, 15);
    }

    #[test]
    fn test_hotspot_summary_builder() {
        let hotspot = HotspotSummary::new("test_function", 100.0, 25.0)
            .with_module("test.dll")
            .with_call_count(100)
            .with_thread_id(1);
        
        assert_eq!(hotspot.function_name, "test_function");
        assert_eq!(hotspot.module_name, Some("test.dll".to_string()));
        assert_eq!(hotspot.call_count, 100);
        assert_eq!(hotspot.thread_id, Some(1));
    }
}
