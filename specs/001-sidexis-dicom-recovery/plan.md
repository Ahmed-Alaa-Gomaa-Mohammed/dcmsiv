# Implementation Plan: SIDEXIS DICOM Recovery & Sorter (dcmsiv)

**Branch**: `001-sidexis-dicom-recovery` | **Date**: 2026-09-19 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/001-sidexis-dicom-recovery/spec.md`

## Summary

Build `dcmsiv`, a high-performance cross-platform CLI tool in Rust specifically engineered to recover, validate, and sort unsorted DICOM archives following hard drive failures on SIDEXIS dental installations. The tool ingests exported `Patient.csv` and `MediaBase.csv` database tables, uses multi-threaded parallel workers (`rayon`) and zero-copy positional seeks (`SeekFrom`) to extract DICOM metadata and calculate middle-layer pixel SHA-1 checksums without loading entire volumetric scans into RAM. Files are safely sorted by default (`move` mode with a `--copy` option) into patient directories (`<InternalCardId>/Volume_<datetime>.dcm` or `RasterImage_<datetime>.dcm`) with strict collision prevention (zero overwrites). An embedded SQLite state database (`.dcmsiv_state.db`) in WAL mode records all transactions, enabling seamless resumption after interruption, atomic rollback (`--undo`), and comprehensive terminal and JSON reporting.

## Technical Context

**Language/Version**: Rust 1.75+ (edition 2021).

**Primary Dependencies**:
- `clap` (v4 with `derive`): CLI argument parsing and validation.
- `rayon` (v1.10+): Data-parallel worker thread pool for scanning, parsing, and hashing.
- `rusqlite` (v0.31+ with `bundled` feature): Embedded local SQLite state database.
- `csv` (v1.3+): High-throughput streaming CSV parser for `Patient.csv` and `MediaBase.csv`.
- `sha1` (v0.10+): Cryptographic hashing of pixel data layers.
- `indicatif` (v0.17+): Interactive, thread-safe terminal progress bar.
- `serde` / `serde_json`: Structured reporting and audit logging.
- `chrono`: Date and time parsing and standard `YYYY-MM-DD_HH-mm-ss` formatting.
- `crossbeam-channel`: Lock-free MPSC channel connecting parallel workers to the SQLite transaction writer thread.

**Storage**: Local embedded SQLite database stored at `<output_dir>/.dcmsiv_state.db` using WAL mode (`PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;`).

**Testing**: Standard Rust test runner (`cargo test`): unit tests, contract validation, and integration tests using 100% synthetic, generated DICOM fixtures.

**Target Platform**: Linux (x86_64, aarch64) and Windows (x86_64) native binaries.

**Project Type**: Standalone CLI executable.

**Performance Goals**:
- Throughput: $\ge 50$ single-layer scans/sec; $\ge 15$ volumetric multi-layer scans/sec on standard SSD storage.
- Resumption speed: $< 3$ seconds to resume an interrupted batch of up to 100,000 files.
- Positional read optimization: $< 2$ MB read per 250 MB volumetric file.

**Constraints**:
- Memory usage strictly bounded $< 500$ MB regardless of dataset size (100 to 100,000+ files).
- Zero Sensitive Data & PHI Leakage: `.gitignore` strictly blocks all `.db`, `.sqlite`, `.dcm`, and clinical data.
- Zero-Overwrite Guarantee: Target files MUST never be overwritten by move or copy operations.
- Crash resilience: Abrupt process termination (SIGINT / SIGKILL) must not corrupt state or files.

**Scale/Scope**: Archives containing up to 100,000+ files and 500+ GB of raw imaging data.

### Layout-Aware DICOM Hashing Architecture (8-bit RGB & Function Updates)

To support SIDEXIS parity for 2D images as documented in `sidexis-import-hash(1).md`, the tool must distinguish pixel layouts and apply layout-specific hashing rules:

#### 1. Header Tags Required for Layer Data Type Identification
- `(0028, 0002)` `SamplesPerPixel` (US): `1` for monochrome, `3` for RGB.
- `(0028, 0004)` `PhotometricInterpretation` (CS): `"RGB"`, `"MONOCHROME2"`, etc.
- `(0028, 0006)` `PlanarConfiguration` (US): `0` (color-by-pixel: `R1 G1 B1 R2 G2 B2`), `1` (color-by-plane).
- `(0028, 0100)` `BitsAllocated` (US): `8` for 8-bit, `16` for 16-bit.
- `(0028, 0008)` `NumberOfFrames` (IS): Frame count (`<= 1` for single-layer raster, `> 1` for CBCT volume).

#### 2. Function Updates & Impact Matrix

| Module | Function / Struct | Proposed Update & Technical Action |
| :--- | :--- | :--- |
| `src/dicom/types.rs` | `struct DicomMetadata` | Add fields: `samples_per_pixel: u16`, `photometric_interpretation: Option<String>`, `planar_configuration: u16`. |
| `src/dicom/header.rs` | `read_dicom_header()` | Add match arms in streaming loop for `(0028, 0002)` (read US `u16`), `(0028, 0004)` (read CS `String`), and `(0028, 0006)` (read US `u16`). Populate new fields on `DicomMetadata`. |
| `src/dicom/hasher.rs` | `compute_pixel_layer_hash()` | Check for 8-bit RGB single layer (`samples_per_pixel == 3 && bits_allocated == 8 && frames <= 1`). When `planar_configuration == 0`, read payload, swap bytes 1 and 3 (`R` and `B`) across `Rows × Columns × 3`, append any trailing pad byte untouched, and hash via SHA-1. |
| `src/dicom/hasher.rs` | `compute_pad_sweep_hashes()` | Add helper returning candidate SHA-1 hashes across all 256 possible trailing pad byte values (`0x00`..`0xFF`) for carved files with altered pad bytes. |
| `src/engine/processor.rs` | `process_candidate_file()` | In `MediaBase` matching step: if direct `computed_hash` fails and the file is an 8-bit RGB raster with odd pixel count, check candidate patient records against pad-swept hashes. |
| `tests/common/synthetic_dicom.rs` | `create_synthetic_rgb_dicom()` | Generate synthetic 8-bit RGB DICOM test files with odd dimensions and trailing pad bytes for deterministic unit/integration testing. |
| `tests/unit/header_tests.rs` | Unit test suite | Add assertions verifying extraction of `samples_per_pixel`, `photometric_interpretation`, and `planar_configuration`. |
| `tests/unit/hasher_tests.rs` | Unit test suite | Add unit tests for 8-bit RGB BGR swapping, pad byte preservation, and 16-bit MONOCHROME2 untouched hashing. |

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Gate Status | Compliance Verification in this Design |
| :--- | :---: | :--- |
| **I. Zero Sensitive Data & PHI Leakage** | **PASS** | State database (`.dcmsiv_state.db`) stored locally in output folder and ignored by root `.gitignore` (`*.db`, `*.sqlite`). Automated tests utilize only synthetic mock DICOM objects. Zero telemetry, completely offline. |
| **II. Cross-Platform Parity (Linux & Windows)** | **PASS** | Implemented in pure Rust with `std::path::PathBuf`, platform-agnostic file moves (`std::fs::rename` with stream copy fallback across devices), bundled SQLite (no external dynamic linking needed), and ANSI/Windows console abstraction (`indicatif`). |
| **III. High Performance & Speed** | **PASS** | `rayon` thread pool for parallel execution; positional reads seek directly to middle frame offset based on header dimensions, avoiding loading 250MB volumes into memory; channel-based SQLite writer batches transactions. |
| **IV. Uncompromising Correctness & DICOM Integrity** | **PASS** | Full DICOM Part 10 verification; strict validation of `InternalCardId` $\leftrightarrow$ `PatientID` and `MediaHash`; mandatory collision checks before moves; idempotent and reversible operations. |
| **V. Explicit & Unambiguous Reporting** | **PASS** | Real-time terminal progress bar; stdout summary on finish; structured `recovery_report.json` and `recovery_report.txt`; distinct exit codes (0, 1, 2, 3, 4); full rollback via `--undo`. |

## Project Structure

### Documentation (this feature)

```text
specs/001-sidexis-dicom-recovery/
├── plan.md              # This implementation plan
├── research.md          # Phase 0 architectural & performance decisions
├── data-model.md        # Phase 1 domain entities & SQLite state schema
├── contracts/
│   └── cli-contract.md  # Phase 1 CLI command flags, exit codes, and output contracts
├── quickstart.md        # Phase 1 runnable end-to-end validation scenarios
└── checklists/
    └── requirements.md  # Specification quality checklist
```

### Source Code Layout (repository root)

```text
Cargo.toml
src/
├── main.rs                   # CLI entry point, signal trapping, and exit code management
├── cli.rs                    # Clap definition of arguments, flags, and validation rules
├── config.rs                 # Runtime configuration and operational mode resolution
├── dicom/
│   ├── mod.rs
│   ├── header.rs             # Streaming DICOM Part 10 header reader & tag extractor
│   ├── hasher.rs             # Positional seek, middle-layer & layout-aware (BGR swap) SHA-1 calculator
│   └── types.rs              # DicomScan, LayerCount, and Tag representations
├── db/
│   ├── mod.rs
│   ├── csv_reader.rs         # Ingestion and indexing of Patient.csv and MediaBase.csv
│   └── state_store.rs        # SQLite state manager (WAL mode, MPSC writer channel)
├── engine/
│   ├── mod.rs
│   ├── processor.rs          # Rayon parallel scanning, matching, and recovery pipeline
│   ├── sorter.rs             # Collision detection, file move/copy execution
│   └── undo.rs               # Transactional rollback engine (--undo)
└── report/
    ├── mod.rs
    ├── progress.rs           # Indicatif progress bar integration
    └── summary.rs            # Terminal stdout report and JSON/txt file generation
tests/
├── common/
│   └── synthetic_dicom.rs    # Deterministic synthetic DICOM generator (zero PHI)
├── unit/
│   ├── header_tests.rs       # Tests for DICOM header parsing and tag extraction
│   ├── hasher_tests.rs       # Tests for positional seek and middle-layer SHA-1 hashing
│   └── csv_tests.rs          # Tests for CSV table parsing and RootNode grouping
└── integration/
    ├── recovery_tests.rs     # End-to-end recovery, sorting, and reporting tests
    ├── resume_tests.rs       # Interruption and resumption tests with SQLite state
    └── undo_tests.rs         # Rollback validation tests via --undo
```

**Structure Decision**: Single Rust workspace with clean module separation (`dicom`, `db`, `engine`, `report`). Isolates low-level binary I/O from SQLite transaction management and CLI reporting, enabling independent unit testing of every subsystem.

## Complexity Tracking

*No constitution violations or unjustified architectural complexity. All gates pass.*

| Component | Design Choice | Simpler Alternative Rejected Because |
| :--- | :--- | :--- |
| Concurrency | Single-writer MPSC Channel + Rayon | Direct multi-threaded SQLite connections cause `SQLITE_BUSY` contention on heavy writes |
| DICOM I/O | Positional seek to middle frame | Full DICOM parsing buffers 250MB files into memory, violating <500MB RAM bound |
| State Management | Embedded SQLite (WAL) | Flat JSON files lack ACID guarantees and corrupt on abrupt power/process termination |
