use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "dcmsiv",
    about = "High-performance SIDEXIS DICOM recovery and sorting tool",
    version
)]
pub struct CliArgs {
    /// Path to directory containing unsorted recovered DICOM files
    #[arg(
        short = 'i',
        long = "input",
        value_name = "INPUT_DIR",
        required_unless_present = "undo"
    )]
    pub input: Option<PathBuf>,

    /// Destination directory for sorted output
    #[arg(
        short = 'o',
        long = "output",
        value_name = "OUTPUT_DIR",
        required_unless_present = "undo"
    )]
    pub output: Option<PathBuf>,

    /// Path to exported Patient.csv table
    #[arg(
        short = 'p',
        long = "patient-csv",
        value_name = "PATH",
        required_unless_present = "undo"
    )]
    pub patient_csv: Option<PathBuf>,

    /// Path to exported MediaBase.csv table
    #[arg(
        short = 'm',
        long = "mediabase-csv",
        value_name = "PATH",
        required_unless_present = "undo"
    )]
    pub mediabase_csv: Option<PathBuf>,

    /// Copy files instead of moving them (default is non-destructive move)
    #[arg(short = 'c', long = "copy", default_value_t = false)]
    pub copy: bool,

    /// Simulate recovery without modifying files on disk
    #[arg(short = 'd', long = "dry-run", default_value_t = false)]
    pub dry_run: bool,

    /// Revert the last sorting operation in the target output directory
    #[arg(short = 'u', long = "undo", value_name = "OUTPUT_DIR")]
    pub undo: Option<PathBuf>,

    /// Delete existing .dcmsiv_state.db and re-evaluate all files from scratch
    #[arg(long = "reset-state", default_value_t = false)]
    pub reset_state: bool,

    /// Number of worker threads (default 0 matches available logical CPU cores)
    #[arg(short = 't', long = "threads", default_value_t = 0)]
    pub threads: usize,

    /// Output terminal report as raw JSON to stdout instead of human table
    #[arg(short = 'j', long = "json", default_value_t = false)]
    pub json: bool,
}

impl CliArgs {
    pub fn parse_args() -> Self {
        Self::parse()
    }
}
