//! 分析结果导出器模块
//!
//! 提供分析结果的导出功能，支持CSV和JSON格式。

use crate::error::{ProfilerError, Result};
use crate::types::{FunctionStats, ProcessStats, ThreadId, ThreadStats};

use super::{CallPath, FlameGraph};

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use tracing::{debug, error, info, trace};

/// 分析结果
#[derive(Debug, Clone)]
pub struct AnalysisResult {
    /// 进程统计信息
    pub process_stats: ProcessStats,
    /// 线程统计映射
    pub thread_stats: HashMap<ThreadId, ThreadStats>,
    /// 火焰图数据
    pub flame_graph: Option<FlameGraph>,
    /// 最耗时的函数列表
    pub top_functions: Vec<FunctionStats>,
    /// 热点调用路径列表
    pub hot_paths: Vec<CallPath>,
    /// 分析开始时间
    pub analysis_start_time: u64,
    /// 分析结束时间
    pub analysis_end_time: u64,
    /// 总样本数
    pub total_samples: u64,
    /// 采样间隔（毫秒）
    pub sample_interval_ms: u32,
}

impl AnalysisResult {
    /// 创建新的分析结果
    pub fn new(process_stats: ProcessStats) -> Self {
        Self {
            process_stats,
            thread_stats: HashMap::new(),
            flame_graph: None,
            top_functions: Vec::new(),
            hot_paths: Vec::new(),
            analysis_start_time: 0,
            analysis_end_time: 0,
            total_samples: 0,
            sample_interval_ms: 1,
        }
    }

    /// 设置火焰图
    pub fn with_flame_graph(mut self, flame_graph: FlameGraph) -> Self {
        self.flame_graph = Some(flame_graph);
        self
    }

    /// 设置分析时间
    pub fn with_timing(mut self, start: u64, end: u64) -> Self {
        self.analysis_start_time = start;
        self.analysis_end_time = end;
        self
    }

    /// 设置采样信息
    pub fn with_sampling_info(mut self, total_samples: u64, interval_ms: u32) -> Self {
        self.total_samples = total_samples;
        self.sample_interval_ms = interval_ms;
        self
    }

    /// 添加线程统计
    pub fn add_thread_stats(&mut self, thread_stats: ThreadStats) {
        self.thread_stats.insert(thread_stats.thread_id, thread_stats);
    }

    /// 添加热点函数
    pub fn add_top_function(&mut self, func_stats: FunctionStats) {
        self.top_functions.push(func_stats);
    }

    /// 添加热点路径
    pub fn add_hot_path(&mut self, path: CallPath) {
        self.hot_paths.push(path);
    }

    /// 获取分析耗时（微秒）
    pub fn analysis_duration_us(&self) -> u64 {
        self.analysis_end_time.saturating_sub(self.analysis_start_time)
    }

    /// 获取总采样时间（毫秒）
    pub fn total_sampling_time_ms(&self) -> u64 {
        self.total_samples * self.sample_interval_ms as u64
    }

    /// 获取指定线程的统计信息
    pub fn get_thread_stats(&self, thread_id: ThreadId) -> Option<&ThreadStats> {
        self.thread_stats.get(&thread_id)
    }

    /// 聚合所有线程的函数统计
    pub fn aggregate_function_stats(&self) -> HashMap<String, FunctionStats> {
        let mut aggregated: HashMap<String, FunctionStats> = HashMap::new();

        for thread in self.thread_stats.values() {
            for (func_name, stats) in &thread.function_stats {
                let entry = aggregated
                    .entry(func_name.clone())
                    .or_insert_with(|| FunctionStats::new(func_name.clone()));
                entry.merge(stats);
            }
        }

        aggregated
    }

    /// 获取最活跃的线程列表
    pub fn top_threads(&self, limit: usize) -> Vec<&ThreadStats> {
        let mut threads: Vec<&ThreadStats> = self.thread_stats.values().collect();
        threads.sort_by(|a, b| b.total_samples.cmp(&a.total_samples));
        threads.into_iter().take(limit).collect()
    }
}

/// 分析导出器 Trait
pub trait AnalysisExporter: Send + Sync {
    /// 导出分析结果
    fn export(&self, result: &AnalysisResult) -> Result<()>;

    /// 设置输出路径
    fn set_output_path(&mut self, path: impl AsRef<Path>);

    /// 获取输出路径
    fn output_path(&self) -> Option<&Path>;
}

/// CSV导出器
#[derive(Debug)]
pub struct CsvExporter {
    output_path: Option<std::path::PathBuf>,
    delimiter: char,
    include_headers: bool,
}

impl Default for CsvExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl CsvExporter {
    /// 创建新的CSV导出器
    pub fn new() -> Self {
        Self {
            output_path: None,
            delimiter: ',',
            include_headers: true,
        }
    }

    /// 创建带配置的CSV导出器
    pub fn with_options(delimiter: char, include_headers: bool) -> Self {
        Self {
            output_path: None,
            delimiter,
            include_headers,
        }
    }

    /// 设置分隔符
    pub fn with_delimiter(mut self, delimiter: char) -> Self {
        self.delimiter = delimiter;
        self
    }

    /// 设置是否包含标题行
    pub fn with_headers(mut self, include: bool) -> Self {
        self.include_headers = include;
        self
    }

    /// 导出函数统计为CSV
    pub fn export_function_stats(
        &self,
        stats: &[&FunctionStats],
        total_time: u64,
        path: impl AsRef<Path>,
    ) -> Result<()> {
        let mut file = std::fs::File::create(path)?;

        if self.include_headers {
            writeln!(
                file,
                "Function{}Module{}Thread{}Total Time (μs){}Self Time (μs){}Call Count{}Avg Time (μs){}Percentage",
                self.delimiter, self.delimiter, self.delimiter,
                self.delimiter, self.delimiter, self.delimiter,
                self.delimiter
            )?;
        }

        for stat in stats {
            let percentage = stat.time_percentage(total_time);
            writeln!(
                file,
                "{}{}{}{}{}{}{}{}{}{}{}{}{}{}{:.2}",
                escape_csv_field(&stat.function_name, self.delimiter),
                self.delimiter,
                stat.module_name.as_deref().unwrap_or("N/A"),
                self.delimiter,
                stat.thread_id.map(|t| t.to_string()).unwrap_or_else(|| "N/A".to_string()),
                self.delimiter,
                stat.total_time_us,
                self.delimiter,
                stat.self_time_us,
                self.delimiter,
                stat.call_count,
                self.delimiter,
                stat.average_time_us,
                self.delimiter,
                percentage
            )?;
        }

        debug!("Exported {} function stats to CSV", stats.len());
        Ok(())
    }

    /// 导出线程统计为CSV
    pub fn export_thread_stats(
        &self,
        stats: &[&ThreadStats],
        total_system_samples: u64,
        path: impl AsRef<Path>,
    ) -> Result<()> {
        let mut file = std::fs::File::create(path)?;

        if self.include_headers {
            writeln!(
                file,
                "Thread ID{}Process ID{}Total Samples{}Function Count{}Total Time (μs){}CPU Usage %",
                self.delimiter, self.delimiter, self.delimiter,
                self.delimiter, self.delimiter
            )?;
        }

        for stat in stats {
            let cpu_usage = stat.cpu_usage_percent(total_system_samples);
            writeln!(
                file,
                "{}{}{}{}{}{}{}{}{}{}{:.2}",
                stat.thread_id,
                self.delimiter,
                stat.process_id,
                self.delimiter,
                stat.total_samples,
                self.delimiter,
                stat.function_stats.len(),
                self.delimiter,
                stat.total_execution_time_us,
                self.delimiter,
                cpu_usage
            )?;
        }

        debug!("Exported {} thread stats to CSV", stats.len());
        Ok(())
    }
}

impl AnalysisExporter for CsvExporter {
    fn export(&self, result: &AnalysisResult) -> Result<()> {
        let output_path = self
            .output_path
            .as_ref()
            .ok_or_else(|| ProfilerError::Generic("Output path not set".to_string()))?;

        info!("Exporting analysis result to CSV: {:?}", output_path);

        let total_time = result.total_sampling_time_ms() * 1000;

        // 导出函数统计
        let func_stats: Vec<&FunctionStats> = result.top_functions.iter().collect();
        let func_path = output_path.with_extension("functions.csv");
        self.export_function_stats(&func_stats, total_time, &func_path)?;

        // 导出线程统计
        let thread_stats: Vec<&ThreadStats> = result.thread_stats.values().collect();
        let thread_path = output_path.with_extension("threads.csv");
        self.export_thread_stats(&thread_stats, result.total_samples, &thread_path)?;

        info!("CSV export completed");
        Ok(())
    }

    fn set_output_path(&mut self, path: impl AsRef<Path>) {
        self.output_path = Some(path.as_ref().to_path_buf());
    }

    fn output_path(&self) -> Option<&Path> {
        self.output_path.as_deref()
    }
}

/// JSON导出器
#[derive(Debug)]
pub struct JsonExporter {
    output_path: Option<std::path::PathBuf>,
    pretty_print: bool,
}

impl Default for JsonExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl JsonExporter {
    /// 创建新的JSON导出器
    pub fn new() -> Self {
        Self {
            output_path: None,
            pretty_print: true,
        }
    }

    /// 创建带配置的JSON导出器
    pub fn with_pretty_print(pretty: bool) -> Self {
        Self {
            output_path: None,
            pretty_print: pretty,
        }
    }

    /// 设置是否美化输出
    pub fn with_pretty(mut self, pretty: bool) -> Self {
        self.pretty_print = pretty;
        self
    }

    /// 序列化分析结果为JSON字符串
    pub fn to_json_string(&self, result: &AnalysisResult) -> Result<String> {
        let json_value = self.to_json_value(result)?;

        if self.pretty_print {
            serde_json::to_string_pretty(&json_value)
                .map_err(|e| ProfilerError::Generic(format!("JSON serialization error: {}", e)))
        } else {
            serde_json::to_string(&json_value)
                .map_err(|e| ProfilerError::Generic(format!("JSON serialization error: {}", e)))
        }
    }

    /// 转换为JSON值
    fn to_json_value(&self, result: &AnalysisResult) -> Result<serde_json::Value> {
        let mut json_obj = serde_json::Map::new();

        // 进程信息
        json_obj.insert(
            "process".to_string(),
            serde_json::json!({
                "process_id": result.process_stats.process_id,
                "process_name": result.process_stats.process_name,
                "total_samples": result.process_stats.total_samples,
            }),
        );

        // 时间信息
        json_obj.insert(
            "timing".to_string(),
            serde_json::json!({
                "analysis_start": result.analysis_start_time,
                "analysis_end": result.analysis_end_time,
                "analysis_duration_us": result.analysis_duration_us(),
                "sampling_interval_ms": result.sample_interval_ms,
                "total_sampling_time_ms": result.total_sampling_time_ms(),
            }),
        );

        // 线程统计
        let thread_stats: Vec<serde_json::Value> = result
            .thread_stats
            .values()
            .map(|ts| {
                let cpu_usage = ts.cpu_usage_percent(result.total_samples);
                serde_json::json!({
                    "thread_id": ts.thread_id,
                    "process_id": ts.process_id,
                    "total_samples": ts.total_samples,
                    "function_count": ts.function_stats.len(),
                    "cpu_usage_percent": cpu_usage,
                })
            })
            .collect();
        json_obj.insert("threads".to_string(), serde_json::Value::Array(thread_stats));

        // 热点函数
        let total_time = result.total_sampling_time_ms() * 1000;
        let top_functions: Vec<serde_json::Value> = result
            .top_functions
            .iter()
            .map(|fs| {
                let percentage = fs.time_percentage(total_time);
                serde_json::json!({
                    "function_name": fs.function_name,
                    "module_name": fs.module_name,
                    "total_time_us": fs.total_time_us,
                    "self_time_us": fs.self_time_us,
                    "call_count": fs.call_count,
                    "percentage": percentage,
                })
            })
            .collect();
        json_obj.insert("top_functions".to_string(), serde_json::Value::Array(top_functions));

        // 热点路径
        let hot_paths: Vec<serde_json::Value> = result
            .hot_paths
            .iter()
            .map(|p| {
                serde_json::json!({
                    "path": p.path_string(),
                    "count": p.count,
                    "total_time_us": p.total_time_us,
                })
            })
            .collect();
        json_obj.insert("hot_paths".to_string(), serde_json::Value::Array(hot_paths));

        // 火焰图
        if let Some(ref flame_graph) = result.flame_graph {
            json_obj.insert(
                "flame_graph".to_string(),
                serde_json::json!({
                    "total_samples": flame_graph.total_samples,
                    "node_count": flame_graph.node_count(),
                    "depth": flame_graph.depth(),
                }),
            );
        }

        Ok(serde_json::Value::Object(json_obj))
    }
}

impl AnalysisExporter for JsonExporter {
    fn export(&self, result: &AnalysisResult) -> Result<()> {
        let output_path = self
            .output_path
            .as_ref()
            .ok_or_else(|| ProfilerError::Generic("Output path not set".to_string()))?;

        info!("Exporting analysis result to JSON: {:?}", output_path);

        let json_string = self.to_json_string(result)?;
        std::fs::write(output_path, json_string)?;

        info!("JSON export completed");
        Ok(())
    }

    fn set_output_path(&mut self, path: impl AsRef<Path>) {
        self.output_path = Some(path.as_ref().to_path_buf());
    }

    fn output_path(&self) -> Option<&Path> {
        self.output_path.as_deref()
    }
}

/// 火焰图导出器
#[derive(Debug)]
pub struct FlameGraphExporter {
    output_path: Option<std::path::PathBuf>,
}

impl Default for FlameGraphExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl FlameGraphExporter {
    /// 创建新的火焰图导出器
    pub fn new() -> Self {
        Self { output_path: None }
    }

    /// 导出火焰图
    pub fn export_flame_graph(&self, flame_graph: &FlameGraph, path: impl AsRef<Path>) -> Result<()> {
        let folded = flame_graph.to_folded_format();
        std::fs::write(path, folded)?;
        debug!("Exported flame graph");
        Ok(())
    }
}

impl AnalysisExporter for FlameGraphExporter {
    fn export(&self, result: &AnalysisResult) -> Result<()> {
        let output_path = self
            .output_path
            .as_ref()
            .ok_or_else(|| ProfilerError::Generic("Output path not set".to_string()))?;

        if let Some(ref flame_graph) = result.flame_graph {
            info!("Exporting flame graph to: {:?}", output_path);
            self.export_flame_graph(flame_graph, output_path)?;
            info!("Flame graph export completed");
            Ok(())
        } else {
            Err(ProfilerError::Generic("No flame graph data available".to_string()))
        }
    }

    fn set_output_path(&mut self, path: impl AsRef<Path>) {
        self.output_path = Some(path.as_ref().to_path_buf());
    }

    fn output_path(&self) -> Option<&Path> {
        self.output_path.as_deref()
    }
}

/// 导出格式枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Json,
    FlameGraph,
}

/// 多功能导出器
#[derive(Debug)]
pub struct MultiExporter {
    csv_exporter: Option<CsvExporter>,
    json_exporter: Option<JsonExporter>,
    flame_exporter: Option<FlameGraphExporter>,
    output_dir: Option<std::path::PathBuf>,
    base_name: String,
}

impl MultiExporter {
    /// 创建新的多功能导出器
    pub fn new() -> Self {
        Self {
            csv_exporter: None,
            json_exporter: None,
            flame_exporter: None,
            output_dir: None,
            base_name: "analysis".to_string(),
        }
    }

    /// 启用CSV导出
    pub fn with_csv(mut self, enabled: bool) -> Self {
        self.csv_exporter = if enabled {
            Some(CsvExporter::new())
        } else {
            None
        };
        self
    }

    /// 启用JSON导出
    pub fn with_json(mut self, enabled: bool) -> Self {
        self.json_exporter = if enabled {
            Some(JsonExporter::new())
        } else {
            None
        };
        self
    }

    /// 启用火焰图导出
    pub fn with_flamegraph(mut self, enabled: bool) -> Self {
        self.flame_exporter = if enabled {
            Some(FlameGraphExporter::new())
        } else {
            None
        };
        self
    }

    /// 设置输出目录
    pub fn with_output_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.output_dir = Some(dir.as_ref().to_path_buf());
        self
    }

    /// 设置基础文件名
    pub fn with_base_name(mut self, name: impl Into<String>) -> Self {
        self.base_name = name.into();
        self
    }

    /// 导出所有启用的格式
    pub fn export_all(&mut self, result: &AnalysisResult) -> Result<()> {
        let output_dir = self
            .output_dir
            .as_ref()
            .ok_or_else(|| ProfilerError::Generic("Output directory not set".to_string()))?;

        std::fs::create_dir_all(output_dir)?;

        if let Some(ref mut exporter) = self.csv_exporter {
            let path = output_dir.join(&self.base_name);
            exporter.set_output_path(&path);
            exporter.export(result)?;
        }

        if let Some(ref mut exporter) = self.json_exporter {
            let path = output_dir.join(format!("{}.json", self.base_name));
            exporter.set_output_path(&path);
            exporter.export(result)?;
        }

        if let Some(ref mut exporter) = self.flame_exporter {
            let path = output_dir.join(format!("{}.folded", self.base_name));
            exporter.set_output_path(&path);
            exporter.export(result)?;
        }

        info!("All exports completed to {:?}", output_dir);
        Ok(())
    }
}

impl Default for MultiExporter {
    fn default() -> Self {
        Self::new()
    }
}

/// 转义CSV字段
fn escape_csv_field(field: &str, delimiter: char) -> String {
    if field.contains(delimiter) || field.contains('"') || field.contains('\n') {
        let escaped = field.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        field.to_string()
    }
}

/// 导出格式解析
pub fn parse_export_format(s: &str) -> Option<ExportFormat> {
    match s.to_lowercase().as_str() {
        "csv" => Some(ExportFormat::Csv),
        "json" => Some(ExportFormat::Json),
        "flamegraph" | "folded" => Some(ExportFormat::FlameGraph),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analysis_result_new() {
        let process_stats = ProcessStats::new(1234);
        let result = AnalysisResult::new(process_stats);

        assert_eq!(result.process_stats.process_id, 1234);
        assert!(result.thread_stats.is_empty());
        assert!(result.top_functions.is_empty());
    }

    #[test]
    fn test_analysis_result_timing() {
        let process_stats = ProcessStats::new(1234);
        let result = AnalysisResult::new(process_stats)
            .with_timing(1000, 5000)
            .with_sampling_info(100, 10);

        assert_eq!(result.analysis_duration_us(), 4000);
        assert_eq!(result.total_sampling_time_ms(), 1000);
    }

    #[test]
    fn test_csv_exporter_new() {
        let exporter = CsvExporter::new();
        assert!(exporter.output_path().is_none());
    }

    #[test]
    fn test_csv_escape_field() {
        assert_eq!(escape_csv_field("simple", ','), "simple");
        assert_eq!(escape_csv_field("with,comma", ','), "\"with,comma\"");
        assert_eq!(escape_csv_field("with\"quote", ','), "\"with\"\"quote\"");
    }

    #[test]
    fn test_json_exporter_new() {
        let exporter = JsonExporter::new();
        assert!(exporter.output_path().is_none());
    }

    #[test]
    fn test_parse_export_format() {
        assert_eq!(parse_export_format("csv"), Some(ExportFormat::Csv));
        assert_eq!(parse_export_format("JSON"), Some(ExportFormat::Json));
        assert_eq!(parse_export_format("FlameGraph"), Some(ExportFormat::FlameGraph));
        assert_eq!(parse_export_format("unknown"), None);
    }

    #[test]
    fn test_multi_exporter_new() {
        let exporter = MultiExporter::new()
            .with_csv(true)
            .with_json(true)
            .with_flamegraph(true)
            .with_base_name("test");

        assert!(exporter.csv_exporter.is_some());
        assert!(exporter.json_exporter.is_some());
        assert!(exporter.flame_exporter.is_some());
        assert_eq!(exporter.base_name, "test");
    }

    #[test]
    fn test_analysis_result_aggregate() {
        let process_stats = ProcessStats::new(1234);
        let mut result = AnalysisResult::new(process_stats);

        let mut thread_stats = ThreadStats::new(1, 1234);
        thread_stats.add_function_stats(FunctionStats::new("func_a"));
        result.add_thread_stats(thread_stats);

        let aggregated = result.aggregate_function_stats();
        assert!(aggregated.contains_key("func_a"));
    }
}
