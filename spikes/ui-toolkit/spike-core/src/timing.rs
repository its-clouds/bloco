//! Startup timing: process start to first frame presented.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Created as the first thing in `main`. A UI arm calls
/// [`StartupTimer::first_frame_presented`] right after its first frame is on screen.
pub struct StartupTimer {
    process_start: Instant,
    first_frame: OnceLock<Duration>,
}

impl StartupTimer {
    pub fn start() -> Self {
        Self {
            process_start: Instant::now(),
            first_frame: OnceLock::new(),
        }
    }

    /// Logs the elapsed time to stderr the first time it is called. Later calls
    /// return the same value without logging again.
    pub fn first_frame_presented(&self) -> Duration {
        *self.first_frame.get_or_init(|| {
            let elapsed = self.process_start.elapsed();
            eprintln!(
                "[spike-core] startup: process start -> first frame presented: {:.1} ms",
                elapsed.as_secs_f64() * 1000.0
            );
            elapsed
        })
    }
}
