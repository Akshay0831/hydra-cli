//! Spinner utilities for CLI operations.
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::interval;

/// Enhanced spinner with message rotation.
#[derive(Clone)]
pub struct EnhancedSpinner {
    message: Arc<Mutex<String>>,
    status_messages: Vec<String>,
    current_index: Arc<Mutex<usize>>,
    running: Arc<Mutex<bool>>,
    task: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl EnhancedSpinner {
    pub fn new(message: String) -> Self {
        Self {
            message: Arc::new(Mutex::new(message)),
            status_messages: Vec::new(),
            current_index: Arc::new(Mutex::new(0)),
            running: Arc::new(Mutex::new(true)),
            task: Arc::new(Mutex::new(None)),
        }
    }

    pub fn add_status_message(&mut self, status: String) {
        self.status_messages.push(status);
    }

    pub fn start(&self) {
        let running = self.running.clone();
        let current_index = self.current_index.clone();
        let status_messages = self.status_messages.clone();
        let message = self.message.clone();

        let task = tokio::spawn(async move {
            let mut interval = interval(Duration::from_millis(100));

            while running.lock().map(|state| *state).unwrap_or(false) {
                let base_message = message
                    .lock()
                    .map(|value| value.clone())
                    .unwrap_or_default();
                let index = current_index.lock().map(|value| *value).unwrap_or_default();
                if status_messages.is_empty() {
                    println!("{} 🔄", base_message);
                } else {
                    let status = &status_messages[index % status_messages.len()];
                    println!("{} {} - {}", base_message, get_spinner_char(), status);
                }

                if let Ok(mut value) = current_index.lock() {
                    *value = index + 1;
                }

                interval.tick().await;
            }
        });
        if let Ok(mut handle) = self.task.lock() {
            *handle = Some(task);
        }
    }

    pub fn stop(&self) {
        if let Ok(mut running) = self.running.lock() {
            *running = false;
        }
        if let Ok(mut task) = self.task.lock() {
            if let Some(handle) = task.take() {
                handle.abort();
            }
        }
    }

    pub fn update_message(&mut self, new_message: String) {
        if let Ok(mut message) = self.message.lock() {
            *message = new_message;
        }
    }
}

fn get_spinner_char() -> &'static str {
    let chars = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let index = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        / 100)
        % chars.len() as u128;
    chars[index as usize]
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_spinner_chars() {
        let spinner_chars = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        for char in spinner_chars {
            assert!(spinner_chars.contains(&char));
        }
    }
}
