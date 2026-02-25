//! Glyph Scheduler - Async task orchestration
//!
//! Manages background tasks like AI operations, file watching, and indexing.

use futures::future::BoxFuture;
use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot, RwLock};
use tracing::{info, warn};

/// Scheduler errors
#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("Task cancelled")]
    Cancelled,
    #[error("Scheduler shutdown")]
    Shutdown,
    #[error("Task not found: {0}")]
    NotFound(TaskId),
}

/// Unique task identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(pub u64);

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TaskId({})", self.0)
    }
}

/// Task priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Background tasks (indexing, etc.)
    Low,
    /// Normal user operations
    Normal,
    /// Time-sensitive (autocomplete, etc.)
    High,
    /// Must complete ASAP (save, etc.)
    Critical,
}

/// Task status
#[derive(Debug, Clone)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed(String),
    Cancelled,
}

/// A handle to a running task
#[derive(Debug)]
pub struct TaskHandle {
    pub id: TaskId,
    cancel_tx: oneshot::Sender<()>,
}

impl TaskHandle {
    /// Cancels the task
    pub fn cancel(self) {
        let _ = self.cancel_tx.send(());
    }
}

/// The task scheduler
pub struct Scheduler {
    next_id: AtomicU64,
    tasks: Arc<RwLock<HashMap<TaskId, TaskStatus>>>,
    #[allow(dead_code)]
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl Scheduler {
    /// Creates a new scheduler
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            tasks: Arc::new(RwLock::new(HashMap::new())),
            shutdown_tx: None,
        }
    }

    /// Spawns a new task with the given priority
    pub fn spawn<F>(&self, _priority: Priority, task: F) -> TaskHandle
    where
        F: FnOnce() -> BoxFuture<'static, Result<(), String>> + Send + 'static,
    {
        let id = TaskId(self.next_id.fetch_add(1, Ordering::SeqCst));
        let (cancel_tx, cancel_rx) = oneshot::channel();
        let task_statuses = Arc::clone(&self.tasks);

        // Mark as pending without blocking the runtime thread.
        if let Ok(mut guard) = task_statuses.try_write() {
            guard.insert(id, TaskStatus::Pending);
        }

        // Spawn the task
        tokio::spawn(async move {
            // Mark as running
            {
                let mut guard = task_statuses.write().await;
                guard.insert(id, TaskStatus::Running);
            }

            // Get the future from the closure
            let future = task();

            // Run with cancellation support
            tokio::select! {
                result = future => {
                    let status = match result {
                        Ok(()) => TaskStatus::Completed,
                        Err(e) => TaskStatus::Failed(e),
                    };
                    let mut guard = task_statuses.write().await;
                    guard.insert(id, status);
                }
                _ = cancel_rx => {
                    info!("Task {:?} cancelled", id);
                    let mut guard = task_statuses.write().await;
                    guard.insert(id, TaskStatus::Cancelled);
                }
            }
        });

        TaskHandle { id, cancel_tx }
    }

    /// Gets the status of a task
    pub async fn status(&self, id: TaskId) -> Option<TaskStatus> {
        let guard = self.tasks.read().await;
        guard.get(&id).cloned()
    }

    /// Shuts down the scheduler
    pub async fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }
        warn!("Scheduler shutdown");
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}
