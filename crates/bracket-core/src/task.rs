//! Long tasks on worker threads: cancelling them and recognising stale results.
//!
//! A task gets a [`Task`]: a cancellation token it checks now and then, and a generation number it
//! sends back with its results. Starting a new task of the same kind cancels the previous one, and
//! results whose generation is not the current one are dropped, even if they arrive after the new
//! task started.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Asks a task to stop. Cheap to clone; every clone shares the same flag.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Tasks check this between units of work and stop early when it is set.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// What a running task needs: its token and its generation.
#[derive(Clone, Debug)]
pub struct Task {
    pub generation: u64,
    pub token: CancelToken,
}

/// Hands out generations for one kind of task (walking a project, a folder search...), so there
/// is at most one current task of that kind.
#[derive(Debug, Default)]
pub struct Generations {
    current: u64,
    token: Option<CancelToken>,
}

impl Generations {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a new task: the previous one is cancelled and its results become stale.
    pub fn start(&mut self) -> Task {
        self.cancel();
        self.current += 1;
        let token = CancelToken::new();
        self.token = Some(token.clone());
        Task {
            generation: self.current,
            token,
        }
    }

    /// Cancels the current task, if any. Its results become stale too.
    pub fn cancel(&mut self) {
        if let Some(token) = self.token.take() {
            token.cancel();
            self.current += 1;
        }
    }

    /// Whether results from `generation` belong to the current task.
    pub fn is_current(&self, generation: u64) -> bool {
        self.token.is_some() && generation == self.current
    }

    /// Whether a task of this kind is running (started and neither cancelled nor finished).
    pub fn is_running(&self) -> bool {
        self.token.is_some()
    }

    /// Marks the current task as finished (its last results arrived). Later results from it are
    /// no longer current.
    pub fn finish(&mut self, generation: u64) {
        if self.is_current(generation) {
            self.token = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_share_the_cancellation() {
        let token = CancelToken::new();
        let copy = token.clone();
        assert!(!copy.is_cancelled());
        token.cancel();
        assert!(copy.is_cancelled());
    }

    #[test]
    fn a_new_task_cancels_the_previous_one() {
        let mut generations = Generations::new();
        let first = generations.start();
        assert!(generations.is_current(first.generation));
        let second = generations.start();
        assert!(first.token.is_cancelled());
        assert!(!second.token.is_cancelled());
        assert!(
            !generations.is_current(first.generation),
            "late results are stale"
        );
        assert!(generations.is_current(second.generation));
    }

    #[test]
    fn cancelling_makes_results_stale() {
        let mut generations = Generations::new();
        let task = generations.start();
        generations.cancel();
        assert!(task.token.is_cancelled());
        assert!(!generations.is_current(task.generation));
        assert!(!generations.is_running());
    }

    #[test]
    fn finishing_ends_the_task_without_cancelling_it() {
        let mut generations = Generations::new();
        let task = generations.start();
        assert!(generations.is_running());
        generations.finish(task.generation);
        assert!(!generations.is_running());
        assert!(!task.token.is_cancelled());
        assert!(!generations.is_current(task.generation));

        // A stale generation cannot finish the current task.
        let current = generations.start();
        generations.finish(task.generation);
        assert!(generations.is_current(current.generation));
    }

    #[test]
    fn a_worker_thread_stops_when_cancelled() {
        let mut generations = Generations::new();
        let task = generations.start();
        let worker = std::thread::spawn(move || {
            let mut units = 0u64;
            while !task.token.is_cancelled() {
                units += 1;
                std::thread::yield_now();
            }
            (task.generation, units)
        });
        std::thread::sleep(std::time::Duration::from_millis(10));
        let newer = generations.start();
        let (generation, _) = worker.join().unwrap();
        assert!(!generations.is_current(generation));
        assert!(generations.is_current(newer.generation));
    }
}
