use std::thread;
use std::time::{Instant, SystemTime};

pub trait Clock {
    fn now_instant(&self) -> Instant;

    fn now_wall(&self) -> SystemTime;

    fn sleep_until(&self, deadline: Instant);
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_instant(&self) -> Instant {
        Instant::now()
    }

    fn now_wall(&self) -> SystemTime {
        SystemTime::now()
    }

    fn sleep_until(&self, deadline: Instant) {
        let now = Instant::now();
        if deadline > now {
            thread::park_timeout(deadline - now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn sleep_until_past_deadline_returns_immediately() {
        let clock = SystemClock;
        let start = Instant::now();
        clock.sleep_until(start - Duration::from_millis(5));
        assert!(start.elapsed() < Duration::from_millis(50));
    }
}
