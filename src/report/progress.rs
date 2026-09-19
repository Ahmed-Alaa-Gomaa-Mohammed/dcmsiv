use indicatif::{ProgressBar, ProgressStyle};
use std::sync::Arc;

#[derive(Clone)]
pub struct RecoveryProgressBar {
    pb: Arc<ProgressBar>,
}

impl RecoveryProgressBar {
    pub fn new(total_files: u64, already_processed: u64) -> Self {
        let pb = ProgressBar::new(total_files);
        let style = ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({percent}%) | {per_sec} | ETA: {eta}")
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("#>-");

        pb.set_style(style);
        pb.set_position(already_processed);

        Self { pb: Arc::new(pb) }
    }

    pub fn inc(&self, delta: u64) {
        self.pb.inc(delta);
    }

    pub fn finish(&self) {
        self.pb.finish_with_message("Scanning and sorting complete.");
    }
}
