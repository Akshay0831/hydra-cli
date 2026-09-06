//! Progress indicators and user feedback for CLI operations.
#![allow(dead_code)]

use crate::spinner::EnhancedSpinner;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

/// CLI utilities for enhanced user feedback and progress tracking.
#[derive(Clone)]
pub struct CliFeedback {
    spinner: Option<EnhancedSpinner>,
    progress_bars: Vec<Arc<ProgressBar>>,
}

impl CliFeedback {
    /// Create a new CLI feedback instance.
    pub fn new() -> Self {
        Self {
            spinner: None,
            progress_bars: Vec::new(),
        }
    }

    /// Create a new spinner for the given message.
    pub fn start_spinner(&mut self, message: String) -> &mut EnhancedSpinner {
        let spinner = EnhancedSpinner::new(message);
        self.spinner = Some(spinner);
        self.spinner.as_mut().unwrap()
    }

    /// Stop the current spinner.
    pub fn stop_spinner(&mut self) {
        self.spinner = None;
        print!("{}\r", " ".repeat(60));
        std::io::Write::flush(&mut std::io::stdout()).unwrap();
    }

    /// Display a success message.
    pub fn success_message(&self, message: String) {
        println!("✅ {}", message);
    }

    /// Display an error message.
    pub fn error_message(&self, message: String) {
        eprintln!("❌ {}", message);
    }

    /// Display an info message.
    pub fn info_message(&self, message: String) {
        println!("ℹ️  {}", message);
    }

    /// Display a warning message.
    pub fn warning_message(&self, message: String) {
        println!("⚠️  {}", message);
    }

    /// Display a progress step.
    pub fn progress_step(&self, step: &str, total: usize, current: usize) {
        let percentage = (current as f64 / total as f64) * 100.0;
        println!("Progress: {:.1}% - {}", percentage, step);
    }

    /// Create a new progress bar.
    pub fn create_progress_bar(&mut self, operation: String, total: usize) -> Arc<ProgressBar> {
        let bar = Arc::new(ProgressBar::new(operation, total));
        self.progress_bars.push(bar.clone());
        bar
    }

    /// Update spinner message.
    pub fn update_spinner(&mut self, message: String) {
        if let Some(spinner) = &mut self.spinner {
            spinner.update_message(message);
        }
    }

    /// Update progress bar.
    pub fn update_progress(&self, index: usize, current: usize, message: Option<String>) {
        if let Some(bar) = self.progress_bars.get(index) {
            if let Some(msg) = message {
                bar.set_message(msg);
            }
            bar.set_current(current);
        }
    }

    /// Complete all progress indicators.
    pub fn complete_all(&mut self) {
        // Complete progress bars
        for bar in self.progress_bars.iter() {
            bar.complete();
        }
        self.progress_bars.clear();

        // Clear spinner
        self.stop_spinner();
    }

    /// Apply formatting to text.
    pub fn formatting(&self, text: &str, style: &str) -> String {
        match style {
            "bold" => format!("**{}**", text),
            "italic" => format!("*{}*", text),
            "code" => format!("`{}`", text),
            _ => text.to_string(),
        }
    }
}

impl Default for CliFeedback {
    fn default() -> Self {
        Self::new()
    }
}

/// Progress information for long-running operations.
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
    pub fn new(operation: String, total: usize) -> Self {
        Self {
            current: 0,
            total,
            operation,
            start_time: Instant::now(),
            estimated_remaining: None,
            message: None,
        }
    }

    pub fn increment(&mut self) {
        self.current = self.current.saturating_add(1);
        self.update_estimate();
    }

    pub fn set_message(&mut self, message: String) {
        self.message = Some(message);
    }

    pub fn set_current(&mut self, current: usize) {
        self.current = current.min(self.total);
        self.update_estimate();
    }

    fn update_estimate(&mut self) {
        if self.total > 0 && self.current > 0 {
            let elapsed = self.start_time.elapsed();
            let rate = self.current as f64 / elapsed.as_secs_f64();

            if rate > 0.0 {
                let remaining_total = (self.total - self.current) as f64;
                let remaining_duration = Duration::from_secs_f64(remaining_total / rate);
                self.estimated_remaining = Some(remaining_duration);
            }
        }
    }

    pub fn percentage(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.current as f64 / self.total as f64) * 100.0
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }
}

/// Progress bar with rich formatting.
#[derive(Clone)]
pub struct ProgressBar {
    progress_info: Arc<Mutex<ProgressInfo>>,
    output_tx: Option<broadcast::Sender<ProgressUpdate>>,
    show_percentage: bool,
    show_eta: bool,
    width: usize,
}

#[derive(Debug, Clone)]
pub enum ProgressUpdate {
    Progress(ProgressInfo),
    Complete,
    Error(String),
}

impl ProgressBar {
    /// Create a new progress bar.
    pub fn new(operation: String, total: usize) -> Self {
        Self {
            progress_info: Arc::new(Mutex::new(ProgressInfo::new(operation, total))),
            output_tx: None,
            show_percentage: true,
            show_eta: true,
            width: 50,
        }
    }

    /// Set whether to show percentage.
    pub fn with_percentage(mut self, show: bool) -> Self {
        self.show_percentage = show;
        self
    }

    /// Set whether to show estimated time remaining.
    pub fn with_eta(mut self, show: bool) -> Self {
        self.show_eta = show;
        self
    }

    /// Set progress bar width.
    pub fn with_width(mut self, width: usize) -> Self {
        self.width = width;
        self
    }

    /// Set output channel for updates.
    pub fn with_output_channel(mut self, tx: broadcast::Sender<ProgressUpdate>) -> Self {
        self.output_tx = Some(tx);
        self
    }

    /// Increment progress.
    pub fn increment(&self) {
        self.progress_info.lock().unwrap().increment();
        self.send_update();
    }

    /// Set current progress.
    pub fn set_current(&self, current: usize) {
        self.progress_info.lock().unwrap().set_current(current);
        self.send_update();
    }

    /// Set progress message.
    pub fn set_message(&self, message: String) {
        self.progress_info.lock().unwrap().set_message(message);
        self.send_update();
    }

    /// Complete the progress bar.
    pub fn complete(&self) {
        {
            let mut info = self.progress_info.lock().unwrap();
            info.current = info.total;
            info.estimated_remaining = Some(Duration::from_secs(0));
            info.message = Some("Completed".to_string());
        }

        if let Some(tx) = &self.output_tx {
            let _ = tx.send(ProgressUpdate::Complete);
        }

        self.print_progress();
        println!();
    }

    /// Send progress update.
    fn send_update(&self) {
        if let Some(tx) = &self.output_tx {
            let info = self.progress_info.lock().unwrap().clone();
            let _ = tx.send(ProgressUpdate::Progress(info));
        }

        self.print_progress();
    }

    /// Print the progress bar.
    fn print_progress(&self) {
        let info = self.progress_info.lock().unwrap();
        let percentage = info.percentage();
        let progress_width = (self.width as f64 * (percentage / 100.0)) as usize;

        let bar: String = if progress_width > 0 {
            "█".repeat(progress_width)
                + "░"
                    .repeat(self.width.saturating_sub(progress_width))
                    .as_str()
        } else {
            "░".repeat(self.width)
        };

        let mut output = format!("{} [{}] {:.1}%", info.operation, bar, percentage);

        if self.show_percentage {
            output.push_str(&format!(" ({}/{})", info.current, info.total));
        }

        if self.show_eta {
            if let Some(eta) = info.estimated_remaining {
                output.push_str(&format!(" ETA: {:.0}s", eta.as_secs_f64()));
            }
        }

        if let Some(message) = &info.message {
            output.push_str(&format!(" - {}", message));
        }

        // Print with carriage return to overwrite the line
        print!("\r{}", output);
        std::io::Write::flush(&mut std::io::stdout()).unwrap();
    }
}

/// Task execution progress tracker.
#[derive(Clone)]
pub struct TaskExecutionProgress {
    completed_tasks: Arc<AtomicUsize>,
    total_tasks: Arc<AtomicUsize>,
    failed_tasks: Arc<AtomicUsize>,
    start_time: Instant,
    progress_bars: Vec<Arc<ProgressBar>>,
}

impl TaskExecutionProgress {
    pub fn new(total_tasks: usize) -> Self {
        Self {
            completed_tasks: Arc::new(AtomicUsize::new(0)),
            total_tasks: Arc::new(AtomicUsize::new(total_tasks)),
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
        if let Some(bar) = self.progress_bars.first() {
            bar.set_message(format!("Task {} completed", task_id));
        }
        self.update_progress();
    }

    pub fn task_failed(&self, task_id: &str, error: &str) {
        self.failed_tasks.fetch_add(1, Ordering::SeqCst);

        if let Some(bar) = self.progress_bars.first() {
            bar.set_message(format!("Task {} failed: {}", task_id, error));
        }

        self.update_progress();
    }

    pub fn update_progress(&self) {
        let completed = self.completed_tasks.load(Ordering::SeqCst);
        let failed = self.failed_tasks.load(Ordering::SeqCst);

        for bar in &self.progress_bars {
            bar.set_current(completed);
            bar.set_message(format!("Completed: {}, Failed: {}", completed, failed));
        }
    }

    pub fn get_statistics(&self) -> TaskExecutionStats {
        TaskExecutionStats {
            completed: self.completed_tasks.load(Ordering::SeqCst),
            total: self.total_tasks.load(Ordering::SeqCst),
            failed: self.failed_tasks.load(Ordering::SeqCst),
            elapsed: self.start_time.elapsed(),
        }
    }

    pub fn complete(self) {
        self.update_progress();

        for bar in self.progress_bars {
            bar.complete();
        }
    }
}

/// Task execution statistics.
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
            Duration::from_millis(0)
        } else {
            self.elapsed / (self.completed as u32)
        }
    }
}

/// Progress manager for coordinating multiple progress indicators.
pub struct ProgressManager {
    progress_bars: Vec<Arc<ProgressBar>>,
    task_execution_progresses: Vec<TaskExecutionProgress>,
    tx: broadcast::Sender<ProgressUpdate>,
}

impl ProgressManager {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(100);
        Self {
            progress_bars: Vec::new(),
            task_execution_progresses: Vec::new(),
            tx,
        }
    }

    pub fn create_progress_bar(&mut self, operation: String, total: usize) -> Arc<ProgressBar> {
        let bar = Arc::new(ProgressBar::new(operation, total).with_output_channel(self.tx.clone()));

        self.progress_bars.push(bar.clone());
        bar
    }

    pub fn create_task_execution_progress(&mut self, total_tasks: usize) -> TaskExecutionProgress {
        let progress = TaskExecutionProgress::new(total_tasks);
        self.task_execution_progresses.push(progress.clone());
        progress
    }

    pub fn get_statistics(&self) -> Vec<TaskExecutionStats> {
        self.task_execution_progresses
            .iter()
            .map(|progress| progress.get_statistics())
            .collect()
    }

    pub fn listen_for_updates(&self) -> broadcast::Receiver<ProgressUpdate> {
        self.tx.subscribe()
    }

    pub fn complete_all(self) {
        for bar in self.progress_bars {
            bar.clone().complete();
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
    fn test_progress_info() {
        let mut progress = ProgressInfo::new("test".to_string(), 100);
        assert_eq!(progress.percentage(), 0.0);

        progress.increment();
        assert_eq!(progress.percentage(), 1.0);

        progress.set_current(50);
        assert_eq!(progress.percentage(), 50.0);
    }

    #[test]
    fn test_progress_bar() {
        let progress = ProgressBar::new("test".to_string(), 100)
            .with_percentage(false)
            .with_eta(false)
            .with_width(20);

        progress.increment();
        progress.set_message("Processing...".to_string());

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
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].completed, 1);
        assert_eq!(stats[0].total, 2);
        assert_eq!(stats[0].failed, 1);
    }
}
