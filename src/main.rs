use chrono::Utc;
use clap::Parser;
use dcmsiv::cli::CliArgs;
use dcmsiv::config::{Config, ExitCode, OperatingMode};
use dcmsiv::db::csv_reader::{parse_mediabase_csv, parse_patient_csv};
use dcmsiv::db::state_store::{BackgroundStateWriter, StateStore};
use dcmsiv::engine::processor::{discover_candidate_files, run_pipeline};
use dcmsiv::engine::undo::execute_undo;
use dcmsiv::report::progress::RecoveryProgressBar;
use dcmsiv::report::summary::RecoveryReport;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

fn main() {
    let args = CliArgs::parse();

    let config = match Config::from_args(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Configuration error: {e}");
            e.exit_code().exit();
        }
    };

    // Set up global Rayon thread pool if requested
    if config.threads > 0 {
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(config.threads)
            .build_global();
    }

    // Set up cross-platform signal handler (SIGINT / SIGTERM)
    let _ = ctrlc::set_handler(move || {
        INTERRUPTED.store(true, Ordering::SeqCst);
        eprintln!("\nOperation interrupted by user signal. Cleaning up...");
        std::process::exit(ExitCode::Interrupted.as_i32());
    });

    match config.mode {
        OperatingMode::Undo { output_dir } => {
            println!("Rolling back operations in {}...", output_dir.display());
            match execute_undo(&output_dir) {
                Ok(restored) => {
                    println!("Undo completed successfully. Restored {restored} files.");
                    ExitCode::Success.exit();
                }
                Err(e) => {
                    eprintln!("Undo failed: {e}");
                    ExitCode::FilesystemIoError.exit();
                }
            }
        }
        OperatingMode::Sort {
            input_dir,
            output_dir,
            patient_csv,
            mediabase_csv,
            copy_mode,
            dry_run,
            reset_state,
        } => {
            let start_time = Utc::now();

            // 1. Ingest and index CSV tables upfront
            let patient_index = match parse_patient_csv(&patient_csv) {
                Ok(idx) => idx,
                Err(e) => {
                    eprintln!("Failed to parse Patient.csv: {e}");
                    ExitCode::DatabaseCsvError.exit();
                }
            };

            let mediabase_index = match parse_mediabase_csv(&mediabase_csv) {
                Ok(idx) => idx,
                Err(e) => {
                    eprintln!("Failed to parse MediaBase.csv: {e}");
                    ExitCode::DatabaseCsvError.exit();
                }
            };

            let total_unique_root_nodes = mediabase_index.total_distinct_root_nodes();

            // 2. Initialize local SQLite state database
            let db_path = output_dir.join(".dcmsiv_state.db");
            let mut already_processed = HashSet::new();

            if !dry_run {
                if reset_state && db_path.exists() {
                    let _ = std::fs::remove_file(&db_path);
                }

                match StateStore::open(&db_path) {
                    Ok(store) => {
                        if reset_state {
                            let _ = store.reset_state();
                        }
                        if let Ok(processed) = store.get_processed_files() {
                            already_processed = processed;
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to initialize state database: {e}");
                        ExitCode::FilesystemIoError.exit();
                    }
                }
            }

            // 3. Start background SQLite transaction writer
            let state_writer = if !dry_run {
                match BackgroundStateWriter::start(db_path.clone()) {
                    Ok(w) => Some(w),
                    Err(e) => {
                        eprintln!("Failed to start background database writer: {e}");
                        ExitCode::FilesystemIoError.exit();
                    }
                }
            } else {
                None
            };

            let (fallback_sender, _fallback_rx) = crossbeam_channel::unbounded();
            let writer_sender = if let Some(ref w) = state_writer {
                w.sender()
            } else {
                fallback_sender
            };

            // 4. Discover candidate files
            let candidates = discover_candidate_files(&input_dir);
            let total_candidates = candidates.len() as u64;
            let already_count = already_processed.len() as u64;

            // 5. Setup progress bar
            let progress_bar = if !config.json {
                Some(RecoveryProgressBar::new(total_candidates, already_count))
            } else {
                None
            };

            let pb_clone = progress_bar.clone();

            // 6. Run parallel pipeline
            let scans = run_pipeline(
                candidates,
                &output_dir,
                &patient_index,
                &mediabase_index,
                &writer_sender,
                &already_processed,
                copy_mode,
                dry_run,
                move |delta, _scan| {
                    if let Some(ref pb) = pb_clone {
                        pb.inc(delta as u64);
                    }
                },
            );

            if let Some(pb) = progress_bar {
                pb.finish();
            }

            // 7. Flush and stop background writer
            if let Some(w) = state_writer {
                if let Err(e) = w.stop() {
                    eprintln!("Warning: Failed to cleanly close state database: {e}");
                }
            }

            // 8. Query recovered RootNode count
            let recovered_unique_root_nodes = if !dry_run && db_path.exists() {
                StateStore::open(&db_path)
                    .and_then(|s| s.get_distinct_recovered_root_nodes_count())
                    .unwrap_or(0)
            } else {
                let mut set = HashSet::new();
                for s in &scans {
                    if let Some(ref rn) = s.matched_root_node {
                        set.insert(rn.clone());
                    }
                }
                set.len()
            };

            // 9. Build and format recovery report
            let end_time = Utc::now();
            let report = RecoveryReport::build(
                config.session_id,
                start_time,
                end_time,
                &input_dir,
                &output_dir,
                copy_mode,
                dry_run,
                &scans,
                total_unique_root_nodes,
                recovered_unique_root_nodes,
            );

            // Write persistent reports if not dry-run
            if !dry_run {
                if let Err(e) = report.write_persistent_reports(&output_dir) {
                    eprintln!("Warning: Failed to write persistent reports: {e}");
                }
            }

            // Output report
            if config.json {
                if let Ok(json_str) = serde_json::to_string_pretty(&report) {
                    println!("{json_str}");
                }
            } else {
                print!("{}", report.format_text_summary());
            }

            ExitCode::Success.exit();
        }
    }
}
