# CLI Interface Contract: dcmsiv

**Feature**: SIDEXIS DICOM Recovery & Sorter  
**Branch**: `001-sidexis-dicom-recovery`  
**Date**: 2026-09-19  

## Command-Line Interface Specification

`dcmsiv` is a command-line binary.

### Usage Syntax

```text
dcmsiv [OPTIONS] --input <INPUT_DIR> --output <OUTPUT_DIR> --patient-csv <PATH> --mediabase-csv <PATH>
dcmsiv --undo <OUTPUT_DIR>
dcmsiv --help
dcmsiv --version
```

### Options & Flags

| Flag / Option | Short | Type | Default | Description |
| :--- | :--- | :--- | :--- | :--- |
| `--input <DIR>` | `-i` | Path | Required* | Path to folder containing unsorted/recovered DICOM files (*unless `--undo`) |
| `--output <DIR>` | `-o` | Path | Required* | Destination directory for sorted output (*unless `--undo`) |
| `--patient-csv <PATH>` | `-p` | File | Required* | Path to exported `Patient.csv` table |
| `--mediabase-csv <PATH>`| `-m` | File | Required* | Path to exported `MediaBase.csv` table |
| `--copy` | `-c` | Flag | `false` | Copy files instead of moving them (default is non-destructive `move`) |
| `--dry-run` | `-d` | Flag | `false` | Simulate recovery without modifying files on disk |
| `--undo <DIR>` | `-u` | Path | None | Revert the last sorting operation in the target output directory |
| `--reset-state` | | Flag | `false` | Delete existing `.dcmsiv_state.db` and re-evaluate all files from scratch |
| `--threads <N>` | `-t` | Integer | `0` (auto) | Number of worker threads (default matches available logical CPU cores) |
| `--json` | `-j` | Flag | `false` | Output terminal report as raw JSON to stdout instead of human table |
| `--help` | `-h` | Flag | | Print help information |
| `--version` | `-V` | Flag | | Print version information |

---

## Exit Codes

| Code | Label | Meaning |
| :---: | :--- | :--- |
| `0` | `SUCCESS` | All files processed or simulation completed successfully |
| `1` | `ARGUMENT_ERROR` | Missing or invalid CLI arguments / flags |
| `2` | `DATABASE_CSV_ERROR` | Input CSV files unreadable, missing required headers, or malformed |
| `3` | `FILESYSTEM_IO_ERROR` | Unable to read input directory, write output, or initialize SQLite database |
| `4` | `INTERRUPTED` | Process terminated prematurely by user signal (SIGINT / SIGTERM) |

---

## Output Contract

### 1. Terminal stdout (Human-Readable Format, Default)

Rendered upon completion of the progress bar:

```text
================================================================================
                      DCMSIV RECOVERY & SORTING REPORT
================================================================================
Session ID:        20260919-054522-a9b1
Input Directory:   /path/to/unsorted
Output Directory:  /path/to/sorted
Mode:              MOVE (Zero-Overwrite Guaranteed)
Execution Time:    00:04:12 (Throughput: 78.4 files/sec)

----------------------------- FILE RECOVERY COUNTS -----------------------------
Total Files Scanned:       12,450
  ✓ Successfully Recovered: 11,820  (94.9%)
  ✗ Corrupt Files Isolated:    210   (1.7%)
  ! Duplicates Segregated:     310   (2.5%)
  ? Unmatched Scans:           110   (0.9%)

----------------------- SIDEXIS DATABASE RECONCILIATION ------------------------
Total Unique MediaBase Files (Distinct RootNodes): 12,500
Recovered Unique MediaBase Files:                 11,820
Overall Archive Recovery Rate:                    94.56%
Estimated Pre-Recovery Carving Loss:                 680 files (5.44%)

-------------------------------- ARTIFACTS SAVED -------------------------------
State Database:    /path/to/sorted/.dcmsiv_state.db
JSON Report:       /path/to/sorted/recovery_report.json
Text Summary:      /path/to/sorted/recovery_report.txt
================================================================================
Recovery complete. All files organized with zero data loss.
```

---

### 2. Structured JSON Report Contract (`recovery_report.json`)

Written to `<output_dir>/recovery_report.json`:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "session": {
    "id": "20260919-054522-a9b1",
    "timestamp_utc": "2026-09-19T05:45:22Z",
    "duration_seconds": 252.4,
    "throughput_files_per_sec": 78.4,
    "mode": "move",
    "dry_run": false
  },
  "metrics": {
    "total_scanned": 12450,
    "recovered": 11820,
    "corrupt": 210,
    "duplicates": 310,
    "unmatched": 110
  },
  "database_reconciliation": {
    "total_mediabase_unique_root_nodes": 12500,
    "recovered_unique_root_nodes": 11820,
    "overall_recovery_percentage": 94.56,
    "estimated_carving_loss": 680
  },
  "paths": {
    "input_directory": "/path/to/unsorted",
    "output_directory": "/path/to/sorted",
    "state_database": "/path/to/sorted/.dcmsiv_state.db"
  }
}
```
