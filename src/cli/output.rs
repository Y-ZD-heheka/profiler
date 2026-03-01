//! 输出格式化模块
//!
//! 实现分析结果的格式化输出、错误信息显示和进程列表显示。

use crate::analysis::AnalysisResult;
use crate::cli::commands::ProcessInfo;
use crate::error::ProfilerError;
use console::style;

/// 打印分析结果摘要
pub fn print_summary(result: &AnalysisResult) {
    println!();
    println!("{}", style("═".repeat(60)).cyan());
    println!(" {}", style("Performance Analysis Report").cyan().bold());
    println!("{}", style("═".repeat(60)).cyan());
    println!();

    // 进程信息
    println!("{}", style("Process Information:").yellow().bold());
    println!("  Process Name: {}", 
        result.process_stats.process_name.as_deref().unwrap_or("Unknown"));
    println!("  Process ID: {}", result.process_stats.process_id);
    println!();

    // 采样统计
    println!("{}", style("Sampling Statistics:").yellow().bold());
    println!("  Total Samples: {}", style(result.total_samples).cyan());
    println!("  Total Threads: {}", style(result.thread_stats.len()).cyan());
    
    let total_functions: usize = result
        .thread_stats
        .values()
        .map(|t| t.function_stats.len())
        .sum();
    println!("  Total Functions: {}", style(total_functions).cyan());
    println!();

    // 热点函数
    if !result.hot_paths.is_empty() {
        println!("{}", style("Top Hotspots:").yellow().bold());
        for (i, path) in result.hot_paths.iter().take(5).enumerate() {
            let percentage = if result.total_samples > 0 {
                (path.count as f64 / result.total_samples as f64) * 100.0
            } else {
                0.0
            };
            
            let path_str = path.path_string();
            let func_name = if path_str.len() > 30 {
                format!("{}...", &path_str[..30])
            } else {
                path_str
            };
            
            println!(
                "  {}. {} ({:.1}%, {} samples)",
                i + 1,
                style(func_name).green(),
                percentage,
                path.count
            );
        }
        println!();
    }

    // 线程摘要
    if !result.thread_stats.is_empty() {
        println!("{}", style("Thread Summary:").yellow().bold());
        let mut threads: Vec<_> = result.thread_stats.values().collect();
        threads.sort_by(|a, b| b.total_samples.cmp(&a.total_samples));
        
        for thread in threads.iter().take(5) {
            let top_funcs = thread.top_functions_by_time(1);
            let top_func = top_funcs
                .first()
                .map(|f| f.simple_name().to_string())
                .unwrap_or_else(|| "N/A".to_string());
            
            println!(
                "  Thread {}: {} samples, top: {}",
                style(thread.thread_id).cyan(),
                style(thread.total_samples).green(),
                style(top_func).yellow()
            );
        }
        println!();
    }

    println!("{}", style("═".repeat(60)).cyan());
}

/// 打印错误信息
pub fn print_error(err: &ProfilerError) {
    eprintln!();
    eprintln!("{}", style("╔".to_string() + &"═".repeat(58) + "╗").red());
    eprintln!("{} {}", 
        style("║").red(),
        style(" ERROR ").red().bold().reverse()
    );
    eprintln!("{}", style("╠".to_string() + &"═".repeat(58) + "╣").red());
    
    // 格式化错误消息
    let msg = err.to_string();
    for line in msg.lines() {
        let wrapped = textwrap::wrap(line, 56);
        for wrap_line in wrapped {
            eprintln!("{} {:56} {}", 
                style("║").red(),
                wrap_line,
                style("║").red()
            );
        }
    }
    
    eprintln!("{}", style("╚".to_string() + &"═".repeat(58) + "╝").red());
    eprintln!();
}

/// 打印进程列表
pub fn print_process_list(processes: &[ProcessInfo]) {
    println!();
    println!("{}", style("Running Processes:").cyan().bold());
    println!("{}", style("─".repeat(80)).cyan());
    println!(
        " {:<10} {:<30} {}",
        style("PID").bold(),
        style("Name").bold(),
        style("Executable Path").bold()
    );
    println!("{}", style("─".repeat(80)).cyan());

    for proc in processes {
        let name = if proc.name.len() > 28 {
            format!("{}...", &proc.name[..25])
        } else {
            proc.name.clone()
        };

        let path = proc
            .exe_path
            .as_ref()
            .map(|p| {
                let s = p.display().to_string();
                if s.len() > 35 {
                    format!("...{}", &s[s.len().saturating_sub(32)..])
                } else {
                    s
                }
            })
            .unwrap_or_else(|| "N/A".to_string());

        println!(" {:<10} {:<30} {}", proc.pid, name, path);
    }

    println!("{}", style("─".repeat(80)).cyan());
    println!("Total: {} processes", style(processes.len()).cyan());
    println!();
}

/// 打印成功消息
pub fn print_success(message: impl AsRef<str>) {
    println!(
        "{} {}",
        style("✓").green().bold(),
        style(message.as_ref()).green()
    );
}

/// 打印警告消息
pub fn print_warning(message: impl AsRef<str>) {
    println!(
        "{} {}",
        style("⚠").yellow().bold(),
        style(message.as_ref()).yellow()
    );
}

/// 打印信息消息
pub fn print_info(message: impl AsRef<str>) {
    println!(
        "{} {}",
        style("ℹ").cyan(),
        style(message.as_ref()).cyan()
    );
}

/// 打印分隔线
pub fn print_separator() {
    println!("{}", style("─".repeat(60)).dim());
}

/// 打印欢迎信息
pub fn print_welcome() {
    println!();
    let version = env!("CARGO_PKG_VERSION");
    println!("{}", style(format!(r#"
╔═══════════════════════════════════════════════════════════╗
║           ETW Performance Profiler v{:<18}       ║
╠═══════════════════════════════════════════════════════════╣
║  A Windows performance profiler based on Event Tracing    ║
║  for Windows (ETW) with CPU sampling and stack walking.   ║
╚═══════════════════════════════════════════════════════════╝
"#, version)).cyan());
    println!();
}

/// 打印帮助提示
pub fn print_help_hint() {
    println!("Use {} for more information", style("--help").yellow());
    println!();
}

/// 打印中断提示
pub fn print_interrupt_hint() {
    println!();
    println!("{}", style("Press Ctrl+C to stop profiling and generate report...").yellow());
    println!();
}

// textwrap 的简单替代实现
mod textwrap {
    pub fn wrap(text: &str, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        let mut current_line = String::new();

        for word in text.split_whitespace() {
            if current_line.len() + word.len() + 1 > width {
                if !current_line.is_empty() {
                    lines.push(current_line.clone());
                    current_line.clear();
                }
                if word.len() > width {
                    // 长单词需要截断
                    let mut start = 0;
                    while start < word.len() {
                        let end = (start + width).min(word.len());
                        lines.push(word[start..end].to_string());
                        start = end;
                    }
                } else {
                    current_line = word.to_string();
                }
            } else {
                if !current_line.is_empty() {
                    current_line.push(' ');
                }
                current_line.push_str(word);
            }
        }

        if !current_line.is_empty() {
            lines.push(current_line);
        }

        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionStats, ProcessStats, ThreadStats};

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
    fn test_print_summary() {
        let result = create_test_analysis_result();
        // 只验证不 panic
        print_summary(&result);
    }

    #[test]
    fn test_print_error() {
        let err = ProfilerError::Generic("Test error".to_string());
        // 只验证不 panic
        print_error(&err);
    }

    #[test]
    fn test_print_process_list() {
        let processes = vec![
            ProcessInfo {
                pid: 1234,
                name: "test.exe".to_string(),
                exe_path: Some(std::path::PathBuf::from("C:\\test.exe")),
            },
        ];
        // 只验证不 panic
        print_process_list(&processes);
    }
}
