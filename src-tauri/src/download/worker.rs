// Download worker module — yt-dlp process management is handled inline in lib.rs.
// This file exists for future expansion (concurrent queue, resume support, etc.)

pub struct WorkerConfig {
    pub max_concurrent: usize,
    pub speed_limit_bps: Option<u64>,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 3,
            speed_limit_bps: None,
        }
    }
}
