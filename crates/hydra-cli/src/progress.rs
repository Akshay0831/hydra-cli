//! Progress indicators and user feedback for CLI operations.

use crate::spinner::EnhancedSpinner;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct CliFeedback {
    spinner: Option<EnhancedSpinner>,
}

impl CliFeedback {
    pub fn new() -> Self {
        Self { spinner: None }
    }

    pub fn start_spinner(&mut self, message: String) {
        let mut spinner = EnhancedSpinner::new(message);
        spinner.add_status_message("working".to_string());
        spinner.start();
        self.spinner = Some(spinner);
    }

    pub fn stop_spinner(&mut self) {
        if let Some(spinner) = self.spinner.take() {
            spinner.stop();
        }
        print!("{}\r", " ".repeat(60));
        std::io::Write::flush(&mut std::io::stdout()).unwrap();
    }

    pub fn update_spinner(&mut self, message: String) {
        if let Some(spinner) = &mut self.spinner {
            spinner.update_message(message);
        }
    }

    pub fn success_message(&self, message: String) {
        println!("✅ {message}");
    }

    pub fn info_message(&self, message: String) {
        println!("ℹ️  {message}");
    }

    pub fn warning_message(&self, message: String) {
        eprintln!("⚠️  {message}");
    }
}

impl Default for CliFeedback {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct ProgressInfo {
    pub current: usize,
    pub total: usize,
    pub operation: String,
    pub start_time: Instant,
    pub estimated_remaining: Option<Duration>,
    pub message: Option<String>,
}

impl ProgressInfo {
    fn new(operation: String, total: usize) -> Self {
        Self {
            current: 0,
            total,
            operation,
            start_time: Instant::now(),
            estimated_remaining: None,
            message: None,
        }
    }

    fn increment(&mut self) {
        self.set_current(self.current.saturating_add(1));
    }

    fn set_message(&mut self, message: String) {
        self.message = Some(message);
    }

    fn set_current(&mut self, current: usize) {
        self.current = current.min(self.total);
        if self.current > 0 && self.total > 0 {
            let elapsed = self.start_time.elapsed().as_secs_f64();
            if elapsed > 0.0 {
                let rate = self.current as f64 / elapsed;
                self.estimated_remaining = Some(Duration::from_secs_f64(
                    (self.total - self.current) as f64 / rate,
                ));
            }
        }
    }

    fn percentage(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.current as f64 / self.total as f64 * 100.0
        }
    }

    fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }
}

#[derive(Clone)]
pub struct ProgressBar {
    progress_info: Arc<Mutex<ProgressInfo>>,
    show_percentage: bool,
    show_eta: bool,
    width: usize,
}

impl ProgressBar {
    fn new(operation: String, total: usize) -> Self {
        Self {
            progress_info: Arc::new(Mutex::new(ProgressInfo::new(operation, total))),
            show_percentage: true,
            show_eta: true,
            width: 40,
        }
    }

    fn with_percentage(mut self, show: bool) -> Self {
        self.show_percentage = show;
        self
    }

    fn with_eta(mut self, show: bool) -> Self {
        self.show_eta = show;
        self
    }

    fn with_width(mut self, width: usize) -> Self {
        self.width = width;
        self
    }

    pub fn increment(&self) {
        let mut info = self
            .progress_info
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        info.increment();
        self.print(&info);
    }

    pub fn set_current(&self, current: usize) {
        let mut info = self
            .progress_info
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        info.set_current(current);
        self.print(&info);
    }

    pub fn set_message(&self, message: String) {
        let mut info = self
            .progress_info
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        info.set_message(message);
        self.print(&info);
    }

    pub fn complete(&self) {
        let mut info = self
            .progress_info
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let total = info.total;
        info.set_current(total);
        info.estimated_remaining = Some(Duration::ZERO);
        let elapsed_ms = info.elapsed().as_millis();
        info.set_message(format!("Completed in {elapsed_ms}ms"));
        self.print(&info);
        println!();
    }

    fn print(&self, info: &ProgressInfo) {
        let completed_width = (self.width as f64 * info.percentage() / 100.0) as usize;
        let bar =
            "█".repeat(completed_width) + &"░".repeat(self.width.saturating_sub(completed_width));
        let percentage = if self.show_percentage {
            format!(" {:.1}%", info.percentage())
        } else {
            String::new()
        };
        let eta = if self.show_eta {
            info.estimated_remaining.map_or(String::new(), |remaining| {
                format!(" ETA {:.1}s", remaining.as_secs_f64())
            })
        } else {
            String::new()
        };
        let message = info
            .message
            .as_deref()
            .map_or(String::new(), |message| format!(" - {message}"));
        println!(
            "{} [{}]{} ({}/{}){}{}",
            info.operation, bar, percentage, info.current, info.total, eta, message
        );
    }
}

#[derive(Clone)]
pub struct TaskExecutionProgress {
    completed_tasks: Arc<AtomicUsize>,
    total_tasks: usize,
    failed_tasks: Arc<AtomicUsize>,
    start_time: Instant,
    progress_bars: Vec<Arc<ProgressBar>>,
}

impl TaskExecutionProgress {
    fn new(total_tasks: usize) -> Self {
        Self {
            completed_tasks: Arc::new(AtomicUsize::new(0)),
            total_tasks,
            failed_tasks: Arc::new(AtomicUsize::new(0)),
            start_time: Instant::now(),
            progress_bars: Vec::new(),
        }
    }

    pub fn add_progress_bar(&mut self, bar: Arc<ProgressBar>) {
        self.progress_bars.push(bar);
    }

    pub fn task_completed(&self, task_id: &str) {
        self.completed_tasks.fetch_add(1, Ordering::SeqCst);
        self.update_progress(format!("Task {task_id} completed"));
    }

    pub fn task_failed(&self, task_id: &str, error: &str) {
        self.failed_tasks.fetch_add(1, Ordering::SeqCst);
        self.update_progress(format!("Task {task_id} failed: {error}"));
    }

    fn update_progress(&self, message: String) {
        let completed = self.completed_tasks.load(Ordering::SeqCst);
        let failed = self.failed_tasks.load(Ordering::SeqCst);
        for bar in &self.progress_bars {
            bar.set_current(completed);
            bar.set_message(format!("{message}; failed: {failed}"));
        }
    }

    pub fn get_statistics(&self) -> TaskExecutionStats {
        TaskExecutionStats {
            completed: self.completed_tasks.load(Ordering::SeqCst),
            total: self.total_tasks,
            failed: self.failed_tasks.load(Ordering::SeqCst),
            elapsed: self.start_time.elapsed(),
        }
    }

    pub fn complete(self) {
        self.update_progress("All tasks complete".to_string());
    }
}

#[derive(Debug, Clone)]
pub struct TaskExecutionStats {
    pub completed: usize,
    pub total: usize,
    pub failed: usize,
    pub elapsed: Duration,
}

impl TaskExecutionStats {
    pub fn success_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.completed.saturating_sub(self.failed) as f64 / self.total as f64 * 100.0
        }
    }

    pub fn average_duration(&self) -> Duration {
        if self.completed == 0 {
            Duration::ZERO
        } else {
            self.elapsed / self.completed as u32
        }
    }
}

pub struct ProgressManager {
    progress_bars: Vec<Arc<ProgressBar>>,
    task_execution_progresses: Vec<TaskExecutionProgress>,
}

impl ProgressManager {
    pub fn new() -> Self {
        Self {
            progress_bars: Vec::new(),
            task_execution_progresses: Vec::new(),
        }
    }

    pub fn create_progress_bar(&mut self, operation: String, total: usize) -> Arc<ProgressBar> {
        let progress = Arc::new(
            ProgressBar::new(operation, total)
                .with_percentage(true)
                .with_eta(true)
                .with_width(40),
        );
        self.progress_bars.push(progress.clone());
        progress
    }

    pub fn create_task_execution_progress(&mut self, total_tasks: usize) -> TaskExecutionProgress {
        let progress = TaskExecutionProgress::new(total_tasks);
        self.task_execution_progresses.push(progress.clone());
        progress
    }

    pub fn get_statistics(&self) -> Vec<TaskExecutionStats> {
        self.task_execution_progresses
            .iter()
            .map(TaskExecutionProgress::get_statistics)
            .collect()
    }

    pub fn complete_all(&self) {
        for bar in &self.progress_bars {
            bar.complete();
        }
        for progress in &self.task_execution_progresses {
            progress.clone().complete();
        }
    }
}

impl Default for ProgressManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_bar_reports_completion() {
        let progress = ProgressBar::new("test".to_string(), 2);
        progress.increment();
        progress.set_message("Processing".to_string());
        progress.complete();
    }

    #[test]
    fn success_rate_does_not_underflow() {
        let stats = TaskExecutionStats {
            completed: 1,
            total: 2,
            failed: 2,
            elapsed: Duration::from_secs(1),
        };
        assert_eq!(stats.success_rate(), 0.0);
    }

    #[test]
    fn manager_reports_task_execution_statistics() {
        let mut manager = ProgressManager::new();
        let progress = manager.create_task_execution_progress(2);
        progress.task_completed("first");
        progress.task_failed("second", "failure");
        let stats = manager.get_statistics();
        assert_eq!(stats[0].completed, 1);
        assert_eq!(stats[0].total, 2);
        assert_eq!(stats[0].failed, 1);
    }
}
