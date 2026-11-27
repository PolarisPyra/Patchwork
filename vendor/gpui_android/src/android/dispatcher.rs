//! Runs GPUI's tasks: the main queue on the activity's looper thread, the rest on worker threads.

// Rust guideline compliant 2026-10-09

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::mpsc;
use std::thread::{self, JoinHandle, ThreadId};
use std::time::{Duration, Instant};

use android_activity::AndroidAppWaker;
use gpui::{
    PlatformDispatcher, Priority, PriorityQueueReceiver, PriorityQueueSender, RunnableVariant,
    profiler,
};

/// The fewest worker threads, even on a one-core device.
const MIN_THREADS: usize = 2;

pub(crate) struct AndroidDispatcher {
    main_sender: PriorityQueueSender<RunnableVariant>,
    waker: AndroidAppWaker,
    timer_sender: mpsc::Sender<Timer>,
    background_sender: PriorityQueueSender<RunnableVariant>,
    _threads: Vec<JoinHandle<()>>,
    main_thread_id: ThreadId,
}

impl AndroidDispatcher {
    /// Starts the worker and timer threads. Call it on the thread that polls the looper.
    pub(crate) fn new(
        main_sender: PriorityQueueSender<RunnableVariant>,
        waker: AndroidAppWaker,
    ) -> Self {
        let (background_sender, background_receiver) = PriorityQueueReceiver::new();
        let thread_count =
            thread::available_parallelism().map_or(MIN_THREADS, |n| n.get().max(MIN_THREADS));

        let mut threads = (0..thread_count)
            .map(|index| {
                let receiver: PriorityQueueReceiver<RunnableVariant> = background_receiver.clone();
                thread::Builder::new()
                    .name(format!("Worker-{index}"))
                    .spawn(move || {
                        for runnable in receiver.iter() {
                            run(runnable);
                        }
                    })
                    .expect("failed to start a GPUI worker thread")
            })
            .collect::<Vec<_>>();

        let (timer_sender, timer_receiver) = mpsc::channel();
        threads.push(
            thread::Builder::new()
                .name("Timer".to_owned())
                .spawn(move || run_timers(&timer_receiver))
                .expect("failed to start the GPUI timer thread"),
        );

        Self {
            main_sender,
            waker,
            timer_sender,
            background_sender,
            _threads: threads,
            main_thread_id: thread::current().id(),
        }
    }
}

impl PlatformDispatcher for AndroidDispatcher {
    fn is_main_thread(&self) -> bool {
        thread::current().id() == self.main_thread_id
    }

    fn dispatch(&self, runnable: RunnableVariant, priority: Priority) {
        if let Err(error) = self.background_sender.send(priority, runnable) {
            // Every worker has exited, so the app is shutting down.
            std::mem::forget(error);
        }
    }

    fn dispatch_on_main_thread(&self, runnable: RunnableVariant, priority: Priority) {
        match self.main_sender.send(priority, runnable) {
            Ok(()) => self.waker.wake(),
            // The main loop has ended. The runnable may hold `!Send` state that must not be
            // dropped off the main thread, so it is leaked.
            Err(error) => std::mem::forget(error),
        }
    }

    fn dispatch_after(&self, duration: Duration, runnable: RunnableVariant) {
        let timer = Timer {
            deadline: Instant::now() + duration,
            runnable,
        };
        if let Err(error) = self.timer_sender.send(timer) {
            // Dropping a scheduled runnable cancels its task and panics its awaiter; leaking it
            // leaves the task pending, which is fine while shutting down.
            std::mem::forget(error);
        }
    }

    fn spawn_realtime(&self, f: Box<dyn FnOnce() + Send>) {
        if let Err(error) = thread::Builder::new().name("Realtime".to_owned()).spawn(f) {
            log::error!("failed to start a realtime thread: {error}");
        }
    }
}

/// Runs one task, recording it for GPUI's profiler.
pub(crate) fn run(runnable: RunnableVariant) {
    let metadata = runnable.metadata();
    profiler::update_running_task(metadata.spawned, metadata.location);
    runnable.run();
    profiler::save_task_timing();
}

struct Timer {
    deadline: Instant,
    runnable: RunnableVariant,
}

impl PartialEq for Timer {
    fn eq(&self, other: &Self) -> bool {
        self.deadline == other.deadline
    }
}

impl Eq for Timer {}

impl PartialOrd for Timer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Timer {
    // Reversed, so the binary heap pops the earliest deadline first.
    fn cmp(&self, other: &Self) -> Ordering {
        other.deadline.cmp(&self.deadline)
    }
}

/// Runs each timer's task when its deadline passes, until the dispatcher is dropped.
fn run_timers(receiver: &mpsc::Receiver<Timer>) {
    let mut pending = BinaryHeap::<Timer>::new();
    loop {
        let now = Instant::now();
        while pending.peek().is_some_and(|timer| timer.deadline <= now) {
            if let Some(timer) = pending.pop() {
                run(timer.runnable);
            }
        }
        let received = match pending.peek() {
            Some(next) => receiver.recv_timeout(next.deadline.saturating_duration_since(now)),
            None => receiver
                .recv()
                .map_err(|mpsc::RecvError| mpsc::RecvTimeoutError::Disconnected),
        };
        match received {
            Ok(timer) => pending.push(timer),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // Leak what is left for the same reason as in `dispatch_after`.
                pending.into_vec().into_iter().for_each(std::mem::forget);
                return;
            }
        }
    }
}
