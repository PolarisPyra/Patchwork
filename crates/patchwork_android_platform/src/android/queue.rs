//! The task queues the dispatcher feeds: one for the looper thread and one the worker threads
//! share. A receiver pops high-priority tasks first, then medium, then low.

// Rust guideline compliant 2026-11-27

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use gpui::Priority;

struct Queues<T> {
    high: VecDeque<T>,
    medium: VecDeque<T>,
    low: VecDeque<T>,
    senders: usize,
    receivers: usize,
}

impl<T> Queues<T> {
    fn pop(&mut self) -> Option<T> {
        self.high
            .pop_front()
            .or_else(|| self.medium.pop_front())
            .or_else(|| self.low.pop_front())
    }

    fn is_empty(&self) -> bool {
        self.high.is_empty() && self.medium.is_empty() && self.low.is_empty()
    }
}

struct Shared<T> {
    queues: Mutex<Queues<T>>,
    ready: Condvar,
}

impl<T> Shared<T> {
    fn lock(&self) -> MutexGuard<'_, Queues<T>> {
        // A task never runs under the lock, so a poisoned lock still holds whole queues.
        self.queues.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The receiving end is gone; holds the item that could not be sent.
pub(crate) struct SendError<T>(pub T);

/// Sends tasks to the receivers of one queue.
pub(crate) struct Sender<T>(Arc<Shared<T>>);

/// Takes tasks from one queue. Clones share the queue, each task going to one of them.
pub(crate) struct Receiver<T>(Arc<Shared<T>>);

/// Makes a queue.
pub(crate) fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let shared = Arc::new(Shared {
        queues: Mutex::new(Queues {
            high: VecDeque::new(),
            medium: VecDeque::new(),
            low: VecDeque::new(),
            senders: 1,
            receivers: 1,
        }),
        ready: Condvar::new(),
    });
    (Sender(Arc::clone(&shared)), Receiver(shared))
}

impl<T> Sender<T> {
    /// Queues `item`. Patchwork plays no realtime audio, so that priority queues as high.
    pub(crate) fn send(&self, priority: Priority, item: T) -> Result<(), SendError<T>> {
        let mut queues = self.0.lock();
        if queues.receivers == 0 {
            return Err(SendError(item));
        }
        match priority {
            Priority::RealtimeAudio | Priority::High => queues.high.push_back(item),
            Priority::Medium => queues.medium.push_back(item),
            Priority::Low => queues.low.push_back(item),
        }
        drop(queues);
        self.0.ready.notify_one();
        Ok(())
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        self.0.lock().senders += 1;
        Self(Arc::clone(&self.0))
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        let mut queues = self.0.lock();
        queues.senders -= 1;
        if queues.senders == 0 {
            drop(queues);
            self.0.ready.notify_all();
        }
    }
}

impl<T> Receiver<T> {
    /// The next task, without waiting.
    pub(crate) fn try_pop(&self) -> Option<T> {
        self.0.lock().pop()
    }

    /// Whether no task waits.
    pub(crate) fn is_empty(&self) -> bool {
        self.0.lock().is_empty()
    }

    /// The next task, waiting for one. `None` once every sender is gone and the queue is empty.
    pub(crate) fn pop(&self) -> Option<T> {
        let mut queues = self.0.lock();
        loop {
            if let Some(item) = queues.pop() {
                return Some(item);
            }
            if queues.senders == 0 {
                return None;
            }
            queues = self
                .0
                .ready
                .wait(queues)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        self.0.lock().receivers += 1;
        Self(Arc::clone(&self.0))
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        self.0.lock().receivers -= 1;
    }
}
