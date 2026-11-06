use std::time::{Duration, Instant};

/// Fixed timestep timekeeper that supports deterministic physics updates
/// and smooth frame interpolation (alpha blending).
#[derive(Debug)]
pub struct GameClock {
    start_time: Instant,
    last_frame_time: Instant,
    accumulated_time: Duration,
    fixed_step: Duration,
    frame_delta: Duration,
    total_time: Duration,
    tick_count: u64,
}

impl GameClock {
    /// Create a new clock with a target fixed physics tick rate (e.g., 60 Hz).
    pub fn new(target_ticks_per_second: u32) -> Self {
        let fixed_step = Duration::from_secs_f64(1.0 / target_ticks_per_second.max(1) as f64);
        let now = Instant::now();
        Self {
            start_time: now,
            last_frame_time: now,
            accumulated_time: Duration::ZERO,
            fixed_step,
            frame_delta: Duration::ZERO,
            total_time: Duration::ZERO,
            tick_count: 0,
        }
    }

    /// Advance the frame timer. Returns the raw frame delta time in seconds.
    pub fn advance_frame(&mut self) -> f32 {
        let now = Instant::now();
        let mut delta = now.duration_since(self.last_frame_time);
        self.last_frame_time = now;

        // Cap maximum frame delta to prevent spiral of death on lag spikes (max 250ms)
        let max_delta = Duration::from_millis(250);
        if delta > max_delta {
            delta = max_delta;
        }

        self.frame_delta = delta;
        self.accumulated_time += delta;
        self.total_time = now.duration_since(self.start_time);
        delta.as_secs_f32()
    }

    /// Checks if a fixed physics step is ready to be executed.
    /// Call this in a `while clock.should_step()` loop.
    pub fn should_step(&mut self) -> bool {
        if self.accumulated_time >= self.fixed_step {
            self.accumulated_time -= self.fixed_step;
            self.tick_count += 1;
            true
        } else {
            false
        }
    }

    /// Fixed physics delta time in seconds (e.g., 0.016667 for 60Hz).
    pub fn fixed_dt(&self) -> f32 {
        self.fixed_step.as_secs_f32()
    }

    /// Interpolation alpha factor [0.0, 1.0] between current and next physics tick.
    /// Used by renderer for buttery-smooth rendering without subpixel jitter.
    pub fn alpha(&self) -> f32 {
        (self.accumulated_time.as_secs_f64() / self.fixed_step.as_secs_f64()) as f32
    }

    /// Total elapsed engine time in seconds.
    pub fn total_seconds(&self) -> f64 {
        self.total_time.as_secs_f64()
    }

    /// Number of fixed ticks processed since start.
    pub fn ticks(&self) -> u64 {
        self.tick_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clock_fixed_dt() {
        let clock = GameClock::new(60);
        let dt = clock.fixed_dt();
        assert!((dt - 0.0166667).abs() < 0.0001);
    }
}
