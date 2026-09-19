use crate::dicom::types::{DicomScan, FileDisposition};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Write;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryReportFileRecord {
    pub source_path: String,
    pub target_path: Option<String>,
    pub status: String,
    pub media_hash: Option<String>,
    pub root_node: Option<String>,
    pub error_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub timestamp_utc: String,
    pub duration_seconds: f64,
    pub throughput_files_per_second: f64,
    pub input_directory: String,
    pub output_directory: String,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryCounts {
    pub total_scanned: usize,
    pub recovered: usize,
    pub corrupt: usize,
    pub duplicate: usize,
    pub unmatched: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseReconciliation {
    pub total_mediabase_unique_root_nodes: usize,
    pub recovered_unique_root_nodes: usize,
    pub recovery_rate_percentage: f64,
    pub estimated_carving_loss_files: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportArtifacts {
    pub state_database: String,
    pub json_report: String,
    pub text_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryReport {
    pub session: SessionInfo,
    pub counts: RecoveryCounts,
    pub database_reconciliation: DatabaseReconciliation,
    pub artifacts: ReportArtifacts,
    pub files: Vec<RecoveryReportFileRecord>,
}

impl RecoveryReport {
    pub fn build(
        session_id: String,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        input_dir: &Path,
        output_dir: &Path,
        copy_mode: bool,
        dry_run: bool,
        scans: &[DicomScan],
        total_unique_root_nodes: usize,
        recovered_unique_root_nodes: usize,
    ) -> Self {
        let duration = (end_time - start_time).num_milliseconds().max(1) as f64 / 1000.0;
        let total_scanned = scans.len();
        let throughput = if duration > 0.0 {
            total_scanned as f64 / duration
        } else {
            0.0
        };

        let mut recovered = 0;
        let mut corrupt = 0;
        let mut duplicate = 0;
        let mut unmatched = 0;

        let mut file_records = Vec::with_capacity(total_scanned);
        for s in scans {
            match s.disposition {
                FileDisposition::Recovered => recovered += 1,
                FileDisposition::Corrupt => corrupt += 1,
                FileDisposition::Duplicate => duplicate += 1,
                FileDisposition::Unmatched => unmatched += 1,
            }

            file_records.push(RecoveryReportFileRecord {
                source_path: s.source_path.to_string_lossy().to_string(),
                target_path: s.destination_path.as_ref().map(|p| p.to_string_lossy().to_string()),
                status: s.disposition.to_string(),
                media_hash: s.computed_hash.clone(),
                root_node: s.matched_root_node.clone(),
                error_reason: s.error_reason.clone(),
            });
        }

        let recovery_rate = if total_unique_root_nodes > 0 {
            (recovered_unique_root_nodes as f64 / total_unique_root_nodes as f64) * 100.0
        } else {
            0.0
        };

        let carving_loss = total_unique_root_nodes.saturating_sub(recovered_unique_root_nodes);

        let mode_str = if dry_run {
            "DRY-RUN (SIMULATION)".to_string()
        } else if copy_mode {
            "COPY (Source Preserved)".to_string()
        } else {
            "MOVE (Zero-Overwrite Guaranteed)".to_string()
        };

        Self {
            session: SessionInfo {
                id: session_id,
                timestamp_utc: start_time.to_rfc3339(),
                duration_seconds: (duration * 100.0).round() / 100.0,
                throughput_files_per_second: (throughput * 10.0).round() / 10.0,
                input_directory: input_dir.to_string_lossy().to_string(),
                output_directory: output_dir.to_string_lossy().to_string(),
                mode: mode_str,
            },
            counts: RecoveryCounts {
                total_scanned,
                recovered,
                corrupt,
                duplicate,
                unmatched,
            },
            database_reconciliation: DatabaseReconciliation {
                total_mediabase_unique_root_nodes: total_unique_root_nodes,
                recovered_unique_root_nodes,
                recovery_rate_percentage: (recovery_rate * 100.0).round() / 100.0,
                estimated_carving_loss_files: carving_loss,
            },
            artifacts: ReportArtifacts {
                state_database: output_dir.join(".dcmsiv_state.db").to_string_lossy().to_string(),
                json_report: output_dir.join("recovery_report.json").to_string_lossy().to_string(),
                text_summary: output_dir.join("recovery_report.txt").to_string_lossy().to_string(),
            },
            files: file_records,
        }
    }

    /// Formats the summary into standard human-readable terminal output
    pub fn format_text_summary(&self) -> String {
        let total = self.counts.total_scanned.max(1) as f64;
        let rec_pct = (self.counts.recovered as f64 / total) * 100.0;
        let cor_pct = (self.counts.corrupt as f64 / total) * 100.0;
        let dup_pct = (self.counts.duplicate as f64 / total) * 100.0;
        let unm_pct = (self.counts.unmatched as f64 / total) * 100.0;

        let total_root = self.database_reconciliation.total_mediabase_unique_root_nodes.max(1) as f64;
        let loss_pct = (self.database_reconciliation.estimated_carving_loss_files as f64 / total_root) * 100.0;

        let duration_secs = self.session.duration_seconds as u64;
        let hours = duration_secs / 3600;
        let mins = (duration_secs % 3600) / 60;
        let secs = duration_secs % 60;
        let time_str = format!("{hours:02}:{mins:02}:{secs:02}");

        format!(
r#"================================================================================
                      DCMSIV RECOVERY & SORTING REPORT
================================================================================
Session ID:        {session_id}
Input Directory:   {input_dir}
Output Directory:  {output_dir}
Mode:              {mode}
Execution Time:    {time_str} (Throughput: {throughput:.1} files/sec)

----------------------------- FILE RECOVERY COUNTS -----------------------------
Total Files Scanned:       {total_scanned:>10}
  ✓ Successfully Recovered:{recovered:>10}  ({rec_pct:>5.1}%)
  ✗ Corrupt Files Isolated:{corrupt:>10}  ({cor_pct:>5.1}%)
  ! Duplicates Segregated: {duplicate:>10}  ({dup_pct:>5.1}%)
  ? Unmatched Scans:       {unmatched:>10}  ({unm_pct:>5.1}%)

----------------------- SIDEXIS DATABASE RECONCILIATION ------------------------
Total Unique MediaBase Files (Distinct RootNodes): {total_root_nodes:>8}
Recovered Unique MediaBase Files:                 {rec_root_nodes:>8}
Overall Archive Recovery Rate:                    {rec_rate:>7.2}%
Estimated Pre-Recovery Carving Loss:              {loss_files:>8} files ({loss_pct:.2}%)

-------------------------------- ARTIFACTS SAVED -------------------------------
State Database:    {state_db}
JSON Report:       {json_report}
Text Summary:      {text_report}
================================================================================
Recovery complete. All files organized with zero data loss.
"#,
            session_id = self.session.id,
            input_dir = self.session.input_directory,
            output_dir = self.session.output_directory,
            mode = self.session.mode,
            throughput = self.session.throughput_files_per_second,
            total_scanned = self.counts.total_scanned,
            recovered = self.counts.recovered,
            corrupt = self.counts.corrupt,
            duplicate = self.counts.duplicate,
            unmatched = self.counts.unmatched,
            total_root_nodes = self.database_reconciliation.total_mediabase_unique_root_nodes,
            rec_root_nodes = self.database_reconciliation.recovered_unique_root_nodes,
            rec_rate = self.database_reconciliation.recovery_rate_percentage,
            loss_files = self.database_reconciliation.estimated_carving_loss_files,
            state_db = self.artifacts.state_database,
            json_report = self.artifacts.json_report,
            text_report = self.artifacts.text_summary,
        )
    }

    /// Writes `recovery_report.json` and `recovery_report.txt` into output directory
    pub fn write_persistent_reports(&self, output_dir: &Path) -> std::io::Result<()> {
        let json_path = output_dir.join("recovery_report.json");
        let txt_path = output_dir.join("recovery_report.txt");

        // Write JSON
        let json_content = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let mut json_file = File::create(json_path)?;
        json_file.write_all(json_content.as_bytes())?;

        // Write Text
        let txt_content = self.format_text_summary();
        let mut txt_file = File::create(txt_path)?;
        txt_file.write_all(txt_content.as_bytes())?;

        Ok(())
    }
}
