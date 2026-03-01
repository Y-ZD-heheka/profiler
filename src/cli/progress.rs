//! 进度显示模块
//!
//! 使用 indicatif crate 显示进度条和进度信息。

use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// 性能分析器进度信息
#[derive(Debug, Clone)]
pub struct ProfilerProgress {
    /// 已收集的样本数
    pub samples_collected: u64,
    /// 已解析的堆栈数
    pub stacks_resolved: u64,
    /// 跟踪的线程数
    pub threads_tracked: usize,
    /// 已用时间
    pub elapsed_time: Duration,
    /// 当前状态描述
    pub status: String,
}

impl Default for ProfilerProgress {
    fn default() -> Self {
        Self {
            samples_collected: 0,
            stacks_resolved: 0,
            threads_tracked: 0,
            elapsed_time: Duration::ZERO,
            status: String::from("Initializing..."),
        }
    }
}

impl ProfilerProgress {
    /// 创建新的进度信息
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置样本数
    pub fn with_samples(mut self, samples: u64) -> Self {
        self.samples_collected = samples;
        self
    }

    /// 设置已解析堆栈数
    pub fn with_resolved(mut self, resolved: u64) -> Self {
        self.stacks_resolved = resolved;
        self
    }

    /// 设置线程数
    pub fn with_threads(mut self, threads: usize) -> Self {
        self.threads_tracked = threads;
        self
    }

    /// 设置已用时间
    pub fn with_elapsed(mut self, elapsed: Duration) -> Self {
        self.elapsed_time = elapsed;
        self
    }

    /// 设置状态
    pub fn with_status(mut self, status: impl Into<String>) -> Self {
        self.status = status.into();
        self
    }
}

/// 进度报告器
pub struct ProgressReporter {
    /// 进度条
    bar: Option<ProgressBar>,
    /// 是否已完成
    finished: AtomicBool,
    /// 开始时间
    start_time: std::time::Instant,
    /// 非交互模式
    non_interactive: bool,
}

impl ProgressReporter {
    /// 创建新的进度报告器
    pub fn new() -> Self {
        Self {
            bar: None,
            finished: AtomicBool::new(false),
            start_time: std::time::Instant::now(),
            non_interactive: false,
        }
    }

    /// 创建非交互模式报告器（无进度条）
    pub fn non_interactive() -> Self {
        Self {
            bar: None,
            finished: AtomicBool::new(false),
            start_time: std::time::Instant::now(),
            non_interactive: true,
        }
    }

    /// 初始化进度条（用于持续时间已知的场景）
    pub fn init_with_duration(&mut self, duration: Duration) {
        if self.non_interactive {
            println!("Profiling for {} seconds...", duration.as_secs());
            return;
        }

        let bar = ProgressBar::new(duration.as_secs());
        bar.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len}s {msg}")
                .expect("Invalid progress template")
                .progress_chars("#>-"),
        );

        self.bar = Some(bar);
    }

    /// 初始化不确定进度条（用于持续时间未知的场景）
    pub fn init_spinner(&mut self, message: impl Into<String>) {
        if self.non_interactive {
            println!("{}", message.into());
            return;
        }

        let bar = ProgressBar::new_spinner();
        bar.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} [{elapsed_precise}] {msg}")
                .expect("Invalid spinner template"),
        );
        bar.enable_steady_tick(Duration::from_millis(100));
        bar.set_message(message.into());

        self.bar = Some(bar);
    }

    /// 更新进度
    pub fn update(&self, progress: &ProfilerProgress) {
        if self.non_interactive {
            return;
        }

        if let Some(ref bar) = self.bar {
            let elapsed_secs = progress.elapsed_time.as_secs();

            // 如果进度条是基于时间的，设置位置
            if bar.length().unwrap_or(0) > 0 {
                bar.set_position(elapsed_secs.min(bar.length().unwrap_or(elapsed_secs)));
            }

            let msg = format!(
                "Samples: {} | Resolved: {} | Threads: {} | {}",
                style(progress.samples_collected).cyan(),
                style(progress.stacks_resolved).green(),
                style(progress.threads_tracked).yellow(),
                progress.status
            );
            bar.set_message(msg);
        }
    }

    /// 更新状态消息
    pub fn set_status(&self, status: impl Into<String>) {
        if self.non_interactive {
            println!("[Status] {}", status.into());
            return;
        }

        if let Some(ref bar) = self.bar {
            bar.set_message(status.into());
        }
    }

    /// 增加进度（用于基于时间的进度条）
    pub fn tick(&self) {
        if let Some(ref bar) = self.bar {
            bar.inc(1);
        }
    }

    /// 完成进度条
    pub fn finish(&self) {
        if self.finished.load(Ordering::SeqCst) {
            return;
        }
        self.finished.store(true, Ordering::SeqCst);

        if let Some(ref bar) = self.bar {
            let total_elapsed = self.start_time.elapsed();
            bar.finish_with_message(format!(
                "Completed in {:.2}s",
                total_elapsed.as_secs_f64()
            ));
        }
    }

    /// 完成并显示成功消息
    pub fn finish_with_success(&self, message: impl Into<String>) {
        if self.finished.load(Ordering::SeqCst) {
            return;
        }
        self.finished.store(true, Ordering::SeqCst);

        if let Some(ref bar) = self.bar {
            bar.finish_with_message(style(message.into()).green().to_string());
        } else {
            println!("{}", style(message.into()).green());
        }
    }

    /// 完成并显示错误消息
    pub fn finish_with_error(&self, message: impl Into<String>) {
        if self.finished.load(Ordering::SeqCst) {
            return;
        }
        self.finished.store(true, Ordering::SeqCst);

        if let Some(ref bar) = self.bar {
            bar.finish_with_message(style(message.into()).red().to_string());
        } else {
            println!("{}", style(message.into()).red());
        }
    }

    /// 获取已用时间
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// 检查是否已完成
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::SeqCst)
    }
}

impl Default for ProgressReporter {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ProgressReporter {
    fn drop(&mut self) {
        self.finish();
    }
}

/// 简单的文本进度报告器（用于非交互式环境）
pub struct TextProgressReporter {
    start_time: std::time::Instant,
    last_report: std::time::Instant,
    report_interval: Duration,
}

impl TextProgressReporter {
    /// 创建新的文本进度报告器
    pub fn new() -> Self {
        let now = std::time::Instant::now();
        Self {
            start_time: now,
            last_report: now,
            report_interval: Duration::from_secs(5),
        }
    }

    /// 报告进度（根据间隔限制输出频率）
    pub fn report(&mut self, progress: &ProfilerProgress) {
        let now = std::time::Instant::now();
        if now.duration_since(self.last_report) >= self.report_interval {
            println!(
                "[{}] Samples: {}, Resolved: {}, Threads: {} - {}",
                format_duration(progress.elapsed_time),
                progress.samples_collected,
                progress.stacks_resolved,
                progress.threads_tracked,
                progress.status
            );
            self.last_report = now;
        }
    }

    /// 强制报告当前进度
    pub fn force_report(&self, progress: &ProfilerProgress) {
        println!(
            "[{}] Samples: {}, Resolved: {}, Threads: {} - {}",
            format_duration(progress.elapsed_time),
            progress.samples_collected,
            progress.stacks_resolved,
            progress.threads_tracked,
            progress.status
        );
    }
}

impl Default for TextProgressReporter {
    fn default() -> Self {
        Self::new()
    }
}

/// 格式化持续时间
fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m {}s", secs / 3600, (secs % 3600) / 60, secs % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_default() {
        let progress = ProfilerProgress::default();
        assert_eq!(progress.samples_collected, 0);
        assert_eq!(progress.stacks_resolved, 0);
        assert_eq!(progress.threads_tracked, 0);
    }

    #[test]
    fn test_progress_builder() {
        let progress = ProfilerProgress::new()
            .with_samples(100)
            .with_resolved(95)
            .with_threads(5)
            .with_status("Testing");

        assert_eq!(progress.samples_collected, 100);
        assert_eq!(progress.stacks_resolved, 95);
        assert_eq!(progress.threads_tracked, 5);
        assert_eq!(progress.status, "Testing");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::from_secs(30)), "30s");
        assert_eq!(format_duration(Duration::from_secs(90)), "1m 30s");
        assert_eq!(format_duration(Duration::from_secs(3665)), "1h 1m 5s");
    }
}
