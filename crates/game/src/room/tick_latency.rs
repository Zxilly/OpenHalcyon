use crate::host::admin_views::{TickLatencyReport, TickStageReport, TickStages};
use std::collections::VecDeque;
use tokio::time::Instant;

const TAIL_US: u64 = 40_000;
const RECENT: usize = 1024;

#[derive(Default)]
pub(crate) struct TickLatency {
    count: u64,
    total_us: u64,
    max_us: u64,
    misses: u64,
    over40: u64,
    pub(crate) recent: VecDeque<u64>,
    stages: [TickStageReport; 3],
}

impl TickLatency {
    pub fn observe(&mut self, scheduled: Instant, wake: Instant, locked: Instant, done: Instant) {
        // Convert all boundaries on the same origin before subtracting so that
        // the three stage durations add up exactly to the reported total.
        let micros = |at: Instant| {
            at.saturating_duration_since(scheduled)
                .as_micros()
                .min(u128::from(u64::MAX)) as u64
        };
        let (wake, locked, us) = (micros(wake), micros(locked), micros(done));
        let sample = [wake, locked - wake, us - locked];

        self.count += 1;
        self.total_us = self.total_us.saturating_add(us);
        self.max_us = self.max_us.max(us);
        self.over40 += u64::from(us > TAIL_US);
        self.misses += u64::from(us > 1_000_000 / halcyon_sim::TICK_HZ);
        if self.recent.len() == RECENT {
            self.recent.pop_front();
        }
        self.recent.push_back(us);

        let mut dominant = 0;
        for (index, stage) in self.stages.iter_mut().enumerate() {
            let duration = sample[index];
            stage.total_us = stage.total_us.saturating_add(duration);
            stage.max_us = stage.max_us.max(duration);
            stage.over40ms += u64::from(duration > TAIL_US);
            if duration > sample[dominant] {
                dominant = index;
            }
        }
        if us > TAIL_US {
            self.stages[dominant].dominant_over40ms += 1;
        }
    }

    pub fn report(&self) -> TickLatencyReport {
        let mut recent: Vec<_> = self.recent.iter().copied().collect();
        recent.sort_unstable();
        let index = (recent.len() * 99).div_ceil(100).saturating_sub(1);
        let [wake_delay, lock_wait, room_work] = self.stages;
        TickLatencyReport {
            count: self.count,
            total_us: self.total_us,
            max_us: self.max_us,
            missed_deadlines: self.misses,
            over40ms: self.over40,
            recent_sample_count: recent.len(),
            p99_us: recent.get(index).copied(),
            stages: TickStages {
                wake_delay,
                lock_wait,
                room_work,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn stages_partition_deadlines_and_tail_attribution() {
        let mut latency = TickLatency::default();
        let scheduled = Instant::now();
        for (wake, locked, done) in [(38, 39, 41), (1, 43, 46), (2, 3, 54), (0, 1, 2)] {
            let after = |ms| scheduled + Duration::from_millis(ms);
            latency.observe(scheduled, after(wake), after(locked), after(done));
        }
        let report = latency.report();
        let stages = report.stages;
        assert_eq!(report.count, 4);
        assert_eq!(report.over40ms, 3);
        assert_eq!(report.missed_deadlines, 1);
        assert_eq!(report.p99_us, Some(54_000));
        assert_eq!(stages.wake_delay.over40ms, 0);
        assert_eq!(stages.lock_wait.over40ms, 1);
        assert_eq!(stages.room_work.over40ms, 1);
        for stage in [stages.wake_delay, stages.lock_wait, stages.room_work] {
            assert_eq!(stage.dominant_over40ms, 1);
        }
        assert_eq!(
            stages.wake_delay.total_us + stages.lock_wait.total_us + stages.room_work.total_us,
            report.total_us
        );
    }
}
