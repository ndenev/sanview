use crate::domain::device::DiskStatistics;

pub struct StatisticsProcessor {
    smoothing_factor: f64,
}

impl StatisticsProcessor {
    pub fn new(smoothing_factor: f64) -> Self {
        Self { smoothing_factor }
    }

    pub fn smooth(&self, current: &DiskStatistics, previous: &DiskStatistics) -> DiskStatistics {
        let alpha = self.smoothing_factor;

        DiskStatistics {
            read_iops: self.apply_smoothing(current.read_iops, previous.read_iops, alpha),
            write_iops: self.apply_smoothing(current.write_iops, previous.write_iops, alpha),
            read_bw_mbps: self.apply_smoothing(current.read_bw_mbps, previous.read_bw_mbps, alpha),
            write_bw_mbps: self.apply_smoothing(current.write_bw_mbps, previous.write_bw_mbps, alpha),
            read_latency_ms: self.apply_smoothing(current.read_latency_ms, previous.read_latency_ms, alpha),
            write_latency_ms: self.apply_smoothing(current.write_latency_ms, previous.write_latency_ms, alpha),
            queue_depth: self.apply_smoothing(current.queue_depth, previous.queue_depth, alpha),
            busy_pct: self.apply_smoothing(current.busy_pct, previous.busy_pct, alpha),
            timestamp: current.timestamp,
        }
    }

    fn apply_smoothing(&self, current: f64, previous: f64, alpha: f64) -> f64 {
        alpha * current + (1.0 - alpha) * previous
    }
}

impl Default for StatisticsProcessor {
    fn default() -> Self {
        Self::new(0.3) // 30% new data, 70% historical
    }
}
