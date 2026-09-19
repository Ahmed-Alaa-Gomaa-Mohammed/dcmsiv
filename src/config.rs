use crate::cli::CliArgs;
use chrono::Utc;
use std::fmt;
use std::path::PathBuf;

/// Standardized, deterministic exit codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ExitCode {
    Success = 0,
    ArgumentError = 1,
    DatabaseCsvError = 2,
    FilesystemIoError = 3,
    Interrupted = 4,
}

impl ExitCode {
    pub fn as_i32(&self) -> i32 {
        *self as i32
    }

    pub fn exit(&self) -> ! {
        std::process::exit(self.as_i32());
    }
}

/// Operational mode of the tool
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperatingMode {
    Sort {
        input_dir: PathBuf,
        output_dir: PathBuf,
        patient_csv: PathBuf,
        mediabase_csv: PathBuf,
        copy_mode: bool,
        dry_run: bool,
        reset_state: bool,
    },
    Undo {
        output_dir: PathBuf,
    },
}

#[derive(Debug)]
pub enum ConfigError {
    MissingArgument(String),
    InputNotFound(PathBuf),
    CsvNotFound { path: PathBuf, kind: &'static str },
    OutputCreateFailed { path: PathBuf, error: String },
    IoError(String),
}

impl ConfigError {
    pub fn exit_code(&self) -> ExitCode {
        match self {
            ConfigError::MissingArgument(_) => ExitCode::ArgumentError,
            ConfigError::CsvNotFound { .. } => ExitCode::DatabaseCsvError,
            ConfigError::InputNotFound(_)
            | ConfigError::OutputCreateFailed { .. }
            | ConfigError::IoError(_) => ExitCode::FilesystemIoError,
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::MissingArgument(arg) => write!(f, "Missing required argument: {arg}"),
            ConfigError::InputNotFound(path) => {
                write!(f, "Input directory not found: {}", path.display())
            }
            ConfigError::CsvNotFound { path, kind } => {
                write!(f, "{kind} CSV file not found: {}", path.display())
            }
            ConfigError::OutputCreateFailed { path, error } => {
                write!(
                    f,
                    "Failed to create output directory {}: {error}",
                    path.display()
                )
            }
            ConfigError::IoError(msg) => write!(f, "Filesystem error: {msg}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Complete resolved runtime configuration
#[derive(Debug, Clone)]
pub struct Config {
    pub mode: OperatingMode,
    pub threads: usize,
    pub json: bool,
    pub session_id: String,
}

impl Config {
    pub fn from_args(args: CliArgs) -> Result<Self, ConfigError> {
        let session_id = format!(
            "{}-{}",
            Utc::now().format("%Y%m%d-%H%M%S"),
            &format!("{:04x}", rand_u16())
        );

        let threads = if args.threads == 0 {
            std::thread::available_parallelism()
                .map(|p| p.get())
                .unwrap_or(4)
        } else {
            args.threads
        };

        if let Some(undo_dir) = args.undo {
            if !undo_dir.exists() {
                return Err(ConfigError::InputNotFound(undo_dir));
            }
            return Ok(Self {
                mode: OperatingMode::Undo {
                    output_dir: undo_dir,
                },
                threads,
                json: args.json,
                session_id,
            });
        }

        let input_dir = args
            .input
            .ok_or_else(|| ConfigError::MissingArgument("--input".to_string()))?;
        let output_dir = args
            .output
            .ok_or_else(|| ConfigError::MissingArgument("--output".to_string()))?;
        let patient_csv = args
            .patient_csv
            .ok_or_else(|| ConfigError::MissingArgument("--patient-csv".to_string()))?;
        let mediabase_csv = args
            .mediabase_csv
            .ok_or_else(|| ConfigError::MissingArgument("--mediabase-csv".to_string()))?;

        if !input_dir.exists() {
            return Err(ConfigError::InputNotFound(input_dir));
        }

        if !patient_csv.exists() {
            return Err(ConfigError::CsvNotFound {
                path: patient_csv,
                kind: "Patient",
            });
        }

        if !mediabase_csv.exists() {
            return Err(ConfigError::CsvNotFound {
                path: mediabase_csv,
                kind: "MediaBase",
            });
        }

        if !args.dry_run && !output_dir.exists() {
            std::fs::create_dir_all(&output_dir).map_err(|e| ConfigError::OutputCreateFailed {
                path: output_dir.clone(),
                error: e.to_string(),
            })?;
        }

        Ok(Self {
            mode: OperatingMode::Sort {
                input_dir,
                output_dir,
                patient_csv,
                mediabase_csv,
                copy_mode: args.copy,
                dry_run: args.dry_run,
                reset_state: args.reset_state,
            },
            threads,
            json: args.json,
            session_id,
        })
    }

    pub fn state_db_path(&self) -> Option<PathBuf> {
        match &self.mode {
            OperatingMode::Sort { output_dir, .. } => Some(output_dir.join(".dcmsiv_state.db")),
            OperatingMode::Undo { output_dir } => Some(output_dir.join(".dcmsiv_state.db")),
        }
    }
}

/// Simple random u16 generator based on timestamp for session ID
fn rand_u16() -> u16 {
    let nanos = Utc::now().timestamp_subsec_nanos();
    (nanos ^ (nanos >> 16)) as u16
}
