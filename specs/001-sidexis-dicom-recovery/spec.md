# Feature Specification: SIDEXIS DICOM Recovery & Sorter (dcmsiv)

**Feature Branch**: `001-sidexis-dicom-recovery`

**Created**: 2026-09-19

**Status**: Draft

**Input**: User description: "dcmsiv is a dicom recovery tool specifcaly designed for SIDEXIS, born from the need to identify and validate a large set of dicom scans after a harddrive failer wiped there original file structure that SIDEXIS used to identify and retrive patient dicom scans, SIDEXIS uses microsoft sql database and has two relavent tables the Patient table containing the PatientId: the internal incremental id used by a SIDEXIS server that is unique per server, InternalCardId: the unique id used by the clinic to identify patients and is unique across all SIDEXIS servers; InteranalCardId in the SIDEXIS database maps to the PatientID tag in the DICOM metadata or fallback to the RETIRED_OtherPatientIDs tag; The second table is the MediaBase table containing which contains the PatientId which maps to the patient table PatientId column; CreationDate which maps to the AcquisitionDateTime dicom tag; MediaHash which is the SHA1 hash of the middle layer of the dicom pixel data if it's a single layer dicom scan then it's the hash of that layer. dcmsiv should cross-refrace the files metadata and hash against the original tables which are in csv format, move corrupt files into a corrupt folder, sort the files under folders with the Patient's identifying unique id and naming them by their AcquisitionDateTime in a human readable format and append the word \"Volume_\" if it's contains multiple layers or \"RasterImage_\" if they only contain one layer, generate a report detailing how many files where recovered, how many are corrupt, how many where duplicates, and have a progress bar. I need the tool to be able to continue after being stopped, it should keep track of the work done by using a light local database so it can recover from failers and contiue after being stopped"

## Clarifications

### Session 2026-09-19
- Q: What is the default file operation mode and safety policy? → A: Default behavior is `move` (with a `--copy` flag to retain source files); destination files MUST never be overwritten by a move operation, with collision detection strictly enforced before executing any move.
- Q: Can the operator revert a sorting operation? → A: Yes, an `--undo` flag MUST be supported to safely roll back the last sorting operation using a recorded transaction journal.
- Q: Where should the recovery summary report be displayed? → A: The comprehensive recovery report MUST be printed directly to the terminal (stdout) upon completion of the progress bar, as well as saved to disk.
- Q: How is overall MediaBase recovery rate calculated when MediaBase contains multiple entries per file? → A: In `MediaBase.csv`, multiple entries per physical file share the same `RootNode` identifier. Total expected media files MUST be calculated as `COUNT(DISTINCT RootNode)`. The report MUST calculate and display overall recovery completeness based on recovered distinct `RootNode` count vs. total distinct `RootNode` count in the database (identifying files lost during the preceding file carving stage).
- Q: How does the tool handle interruption or failure during processing? → A: The tool MUST use a lightweight local embedded database (stored at `<output_dir>/.dcmsiv_state.db`) to track all scanned files, extracted metadata, computed hashes, and operation states in real time; if interrupted (manually via SIGINT or by system crash), subsequent runs detect the database and seamlessly resume processing without duplicating work or re-hashing completed files.
- Q: Where is transaction and rollback state stored? → A: Transaction logging and rollback manifests are unified exclusively in the SQLite `.dcmsiv_state.db` database (`transactions` table), eliminating standalone JSON undo files.
- Q: What format is the DICOM pixel data stored in? → A: The pixel data is confirmed to be strictly raw and uncompressed, allowing direct byte-offset positional seeks to the target middle/single layer without decompression overhead.
- Q: What exit code is returned when processing is interrupted? → A: The tool returns exit code 4 (`INTERRUPTED`) when terminated by a user signal (SIGINT / SIGTERM), achieving complete deterministic parity with the CLI contract.
- Q: How does SIDEXIS compute the MediaHash for 8-bit RGB 2D single-layer raster images? → A: For 8-bit RGB interleaved images (`PlanarConfiguration = 0`), SIDEXIS reconstructs the pixel data into a **Windows DIB (Device Independent Bitmap) memory layout** before hashing: (1) swap the red and blue channels so each pixel is stored in BGR byte order, (2) pad each row to a **4-byte (DWORD) boundary** with `0x00` bytes (pad per row = `(4 - (Columns × 3) % 4) % 4`), (3) rows are stored top-to-bottom (no vertical flip), (4) compute SHA-1 over the resulting DIB pixel buffer. The DICOM trailing pad byte is **not** included in the hash — the hashed buffer is `Rows × (Columns × 3 + row_padding)` bytes. If matching against stored copies, an optional 256-pad sweep is no longer needed since the hash is computed from the decoded pixel array, not the raw Pixel Data element value.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Core DICOM Identification, Verification & Sorting (Priority: P1)

As a dental clinic IT administrator recovering from hard drive failure, I want to point `dcmsiv` at an unsorted dump of recovered DICOM files alongside exported SIDEXIS database tables (`Patient.csv` and `MediaBase.csv`), so that valid DICOM scans are identified, verified against patient records, and organized into structured, human-readable patient directories via non-destructive move operations by default.

**Why this priority**: This represents the foundational MVP functionality. Without cross-referencing metadata, computing pixel layer hashes, and sorting files into patient folders, no patient images can be recovered or used by clinical staff.

**Independent Test**: Can be tested with a small set of synthetic DICOM files (both single-layer and multi-layer) and corresponding CSV entries. Verifies that files are moved into `<output_dir>/<InternalCardId>/` with the expected prefix (`Volume_` or `RasterImage_`) and timestamp, with zero file overwrites.

**Acceptance Scenarios**:

1. **Given** an unsorted directory containing a valid single-layer DICOM scan, a `Patient.csv` linking `InternalCardId` to `PatientId`, and a `MediaBase.csv` containing matching `PatientId`, `CreationDate`, and single-layer `MediaHash`, **When** `dcmsiv` processes the directory without flags, **Then** the file is moved to `<output>/<InternalCardId>/RasterImage_<AcquisitionDateTime>.dcm`, the original source file is no longer in the input directory, and it is marked as recovered.
2. **Given** the `--copy` flag is passed on the command line, **When** `dcmsiv` processes the scan, **Then** the source file is preserved in the input directory and a copy is created in the patient directory.
3. **Given** a valid multi-layer volumetric DICOM scan (CBCT volume), **When** `dcmsiv` processes the scan, **Then** it calculates the SHA-1 hash of the middle pixel layer, matches it against `MediaBase.csv`, and moves the file into `<output>/<InternalCardId>/Volume_<AcquisitionDateTime>.dcm`.
4. **Given** a DICOM file where the `PatientID` (0010,0020) tag is missing or blank but `RETIRED_OtherPatientIDs` (0010,1000) matches an `InternalCardId` in `Patient.csv`, **When** `dcmsiv` processes the file, **Then** it uses the fallback tag to successfully resolve the patient and sort the file.

---

### User Story 2 - Quarantine of Corrupt, Duplicate & Unmatched Files (Priority: P2)

As a dental clinic technician, I want unreadable or non-matching files safely separated from successfully recovered patient files, so that clinical staff never open corrupted scans and duplicate files do not overwrite each other.

**Why this priority**: Prevents silent data loss and protects clinical safety by isolating damaged files into a dedicated triage area while preserving complete data integrity.

**Independent Test**: Can be tested by feeding truncated files, invalid non-DICOM binaries, duplicate copies of the same scan, and orphan DICOMs with unknown patient IDs, verifying each lands in its respective isolation folder (`corrupt/`, `duplicates/`, or `unmatched/`) without overwriting existing files.

**Acceptance Scenarios**:

1. **Given** a truncated or corrupted file with an unreadable header or broken pixel stream, **When** `dcmsiv` encounters the file, **Then** the file is moved into `<output>/corrupt/<original_filename>`, the specific error is logged, and the corrupt file counter increments.
2. **Given** two identical DICOM files (matching the same patient and `MediaHash`), **When** `dcmsiv` processes both files, **Then** the first instance is sorted into the patient directory, the second instance is safely moved into `<output>/duplicates/` (or given a collision-free name), ensuring neither file is overwritten, and the duplicate counter increments.
3. **Given** a valid DICOM scan whose `PatientID` / `OtherPatientIDs` or `MediaHash` cannot be found in the provided CSV tables, **When** `dcmsiv` processes the file, **Then** the file is moved into `<output>/unmatched/` for manual review, rather than being marked as corrupt.

---

### User Story 3 - Progress Indication & Comprehensive Terminal/File Reporting (Priority: P3)

As a systems operator running recovery over tens of thousands of imaging files, I want visual progress monitoring during execution and a comprehensive report printed directly to my terminal upon completion, so that I know processing throughput, remaining time, and overall database recovery completeness against original `MediaBase` files.

**Why this priority**: Essential for large clinical datasets where recovery runs for hours. Operators need live visibility into progress and an immediate on-screen audit confirming how many original `MediaBase` files were recovered versus lost in earlier carving stages.

**Independent Test**: Can be tested by running `dcmsiv` on a sample batch and verifying an interactive progress bar renders on the terminal, followed immediately by an on-screen terminal summary report that details total scanned, recovered, corrupt, duplicates, unmatched, and distinct `RootNode` recovery percentages against `MediaBase`.

**Acceptance Scenarios**:

1. **Given** an ongoing recovery batch of 5,000 files, **When** `dcmsiv` executes, **Then** the terminal displays a live progress bar showing total files, processed count, percentage, files per second, and elapsed/estimated time.
2. **Given** a completed recovery run, **When** all files have been processed, **Then** `dcmsiv` prints a complete recovery report directly to the terminal (stdout) detailing:
   - Total files scanned, recovered, corrupt, duplicates, and unmatched
   - Total distinct `RootNode` media files in `MediaBase.csv`
   - Total distinct `RootNode` media files successfully recovered
   - Overall recovery percentage (`Recovered RootNodes / Total MediaBase RootNodes * 100%`) highlighting files lost during prior file carving
3. **Given** completion of the run, **When** terminal output finishes, **Then** persistent copies of the report (`recovery_report.json` and `recovery_report.txt`) are also written to `<output_dir>`.

---

### User Story 4 - Non-Destructive Preview / Dry-Run Execution (Priority: P4)

As a cautious system administrator, I want to run `dcmsiv` in simulation mode before modifying any files on disk, so that I can inspect the recovery plan, check match rates against the database, and identify issues without risking disk writes.

**Why this priority**: Safeguards clinical files before irreversible operations, allowing validation of CSV mappings and disk capacity requirements upfront.

**Independent Test**: Can be tested by supplying `--dry-run` flag; verifies that no files are created, moved, or deleted in the destination folder, but the recovery report accurately forecasts all planned moves.

**Acceptance Scenarios**:

1. **Given** `--dry-run` is specified on the command line, **When** `dcmsiv` runs, **Then** all files are evaluated and matched, the progress bar and terminal summary report are generated, but no files are moved, copied, or modified on disk.

---

### User Story 5 - Reverting Operations via Undo Flag (Priority: P2)

As a systems operator who made a mistake in output path selection or configuration, I want to run `dcmsiv --undo`, so that all moved files from the last sorting operation are safely and automatically returned to their original source locations.

**Why this priority**: Moving thousands of files can be disruptive if the wrong destination is provided or if configuration parameters were mismatched. An automated undo prevents manual recovery agony and eliminates risk.

**Independent Test**: Can be tested by running a sorting operation on a test folder in default `move` mode, verifying files moved, and then executing `dcmsiv --undo <output_dir>`, confirming that all sorted, corrupt, duplicate, and unmatched files are returned to their exact original pre-move paths.

**Acceptance Scenarios**:

1. **Given** a previously completed `dcmsiv` run in `move` mode with a valid transaction journal in the state database, **When** the user executes `dcmsiv --undo <output_dir>`, **Then** each moved file is restored to its original source path and the transaction journal is marked as rolled back.
2. **Given** an undo request where destination files have been manually altered or deleted since the run, **When** `dcmsiv --undo` executes, **Then** it identifies missing/altered files, restores all intact files, and reports detailed warnings for any discrepancies without failing silently.

---

### User Story 6 - Interruption Recovery & Resumable Execution (Priority: P1)

As a systems operator processing a multi-hundred-gigabyte archive over several hours, I want the tool to save its progress into a lightweight local database so that if the process is paused, interrupted (Ctrl+C), or interrupted by a system crash, I can simply re-run the tool and have it pick up right where it left off without re-processing or corrupting files.

**Why this priority**: Vital for production recovery operations on large datasets. Without resumption, any interruption requires wiping the output directory and restarting hours of heavy I/O from scratch.

**Independent Test**: Can be tested by initiating a recovery batch, terminating the process with SIGINT (or abrupt kill) midway through, re-executing `dcmsiv` with the same arguments, and verifying that the tool skips already processed files and resumes processing only the remaining queue to 100% completion.

**Acceptance Scenarios**:

1. **Given** an in-progress recovery process that is interrupted by user signal (SIGINT) or unexpected crash, **When** the operator re-executes `dcmsiv` targeting the same directories, **Then** the tool detects the existing `.dcmsiv_state.db`, loads previously processed records, skips re-hashing and re-moving completed files, and resumes from the first unfinished file.
2. **Given** a resumed execution, **When** the tool finishes, **Then** the terminal progress bar and final recovery report accurately reflect the aggregate counts across all sessions without duplicate counting.
3. **Given** an operator who explicitly desires to discard prior progress and restart clean, **When** passing the `--reset-state` flag, **Then** the existing state database is cleared and recovery begins afresh.

---

### Edge Cases

- **Destination File Collision (Zero-Overwrite Guarantee)**: Under no circumstances may any existing file in the target directory be overwritten by a move or copy operation. If a destination path already exists:
  - If the file is identical (same SHA-1 media hash), it is treated as a duplicate and segregated into `duplicates/`.
  - If the file has a different hash, a deterministic incremental suffix (e.g., `_01`, `_02`) MUST be appended to the filename to preserve both files.
- **Multiple Entries per File in MediaBase (RootNode Grouping)**: In SIDEXIS Microsoft SQL Server, the `MediaBase` table contains multiple records per physical DICOM file, representing different image processing states or annotations. All records corresponding to the same physical file share identical `RootNode` values. To accurately report overall recovery rate, the tool MUST group `MediaBase` by `RootNode` and count distinct `RootNodes`.
- **Interrupted Move Reconciliation on Resume**: If a process crash occurs immediately after a physical file move but before the database transaction commits, on resume the tool checks if the destination file exists with the expected hash; if verified, it records the completed state without attempting to re-move from the now-empty source location.
- **State Database Crash Resilience**: The state database MUST operate in Write-Ahead Logging (WAL) or equivalent crash-safe transaction mode to ensure that power loss or abrupt process termination cannot corrupt the database file.
- **Modified Input Files Between Sessions**: If an input file's size or timestamp changes between an interrupted session and a resumed session, the tool MUST invalidate the cached record for that specific file and re-evaluate it.
- **Missing Both PatientID and OtherPatientIDs**: If a DICOM file has neither `PatientID` nor `RETIRED_OtherPatientIDs`, the tool attempts to match by `MediaHash` against `MediaBase.csv`. If matched to a single `PatientId`, it resolves the corresponding `InternalCardId` from `Patient.csv`; otherwise, it routes the file to `unmatched/`.
- **Missing or Partial AcquisitionDateTime**: If `AcquisitionDateTime` (0008,002A) is empty or absent, the tool falls back sequentially to:
  1. `AcquisitionDate` (0008,0022) + `AcquisitionTime` (0008,0032)
  2. `SeriesDate` (0008,0021) + `SeriesTime` (0008,0031)
  3. `CreationDate` matched from the `MediaBase.csv` record
  4. Placeholder `UnknownDate` if no valid timestamp can be determined.
- **Even vs. Odd Frame Count for Volumetric Scans**: For a multi-layer volume with $N$ frames (0-indexed), the middle layer index is defined deterministically as $\lfloor N / 2 \rfloor$ (e.g., frame 100 for a 200-frame volume, frame 100 for a 201-frame volume).
- **Corrupted or Truncated Middle Frame**: If a volumetric DICOM file's header claims $N$ frames, but the file is truncated before the middle frame can be decoded, the file MUST be classified as `corrupt` and moved to `corrupt/`.
- **Invalid Characters in Patient Identifiers**: If `InternalCardId` contains characters prohibited by Windows or POSIX filesystems (`/`, `\`, `:`, `*`, `?`, `"`, `<`, `>`, `|`), the tool MUST sanitize these characters into safe substitutes (e.g., `_`) for directory creation while preserving the original identifier in audit logs.
- **Malformed or Incomplete CSV Rows**: If `Patient.csv` or `MediaBase.csv` contains unparseable lines, duplicate primary keys, or missing required columns, `dcmsiv` MUST report the specific row errors and halt with a descriptive exit code before starting file manipulation.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The CLI MUST accept input parameters specifying:
  - Input directory containing unsorted/recovered DICOM files
  - Path to exported `Patient.csv`
  - Path to exported `MediaBase.csv`
  - Output destination directory
  - Processing mode: `move` by default; optional `--copy` flag to preserve source files
  - Optional `--dry-run` flag to simulate operations without modifying disk state
  - Optional `--undo` flag to revert the last completed sorting operation
  - Optional `--reset-state` flag to clear any existing progress database and start afresh
- **FR-002**: The tool MUST parse and index `Patient.csv` upfront, validating the presence of `PatientId` and `InternalCardId` columns.
- **FR-003**: The tool MUST parse and index `MediaBase.csv` upfront, validating the presence of `PatientId`, `CreationDate`, `MediaHash`, and `RootNode` columns.
- **FR-004**: For each candidate DICOM file, the tool MUST read DICOM metadata, extracting `PatientID` (0010,0020), `RETIRED_OtherPatientIDs` (0010,1000), `AcquisitionDateTime` (0008,002A), and frame count / dimensions.
- **FR-005**: If `PatientID` is absent or whitespace-only, the tool MUST fall back to `RETIRED_OtherPatientIDs` to resolve the patient's `InternalCardId`.
- **FR-006**: The tool MUST determine whether each DICOM file is single-layer (raster image, 1 frame) or multi-layer (volumetric scan, >1 frame).
- **FR-007**: The tool MUST compute the SHA-1 hash of pixel data according to image layout:
  - For single-layer 16-bit monochrome (`MONOCHROME2`) scans: SHA-1 hash of the raw pixel byte buffer as stored.
  - For single-layer 8-bit RGB scans with `PlanarConfiguration = 0` (interleaved): reconstruct the decoded pixel array into a **Windows DIB memory layout** — BGR channel order with each row padded to a 4-byte (DWORD) boundary (`pad_per_row = (4 - (Columns × 3) % 4) % 4`), rows top-to-bottom — then compute SHA-1 over the resulting `Rows × (Columns × 3 + pad_per_row)` byte buffer.
  - For multi-layer scans: SHA-1 hash of the middle layer raw pixel byte buffer, where the middle layer index is $\lfloor \text{NumberOfFrames} / 2 \rfloor$.
- **FR-008**: The tool MUST cross-reference the extracted metadata and computed hash against `MediaBase.csv` and `Patient.csv`:
  - Verify patient identity matches `InternalCardId` $\leftrightarrow$ `PatientId`
  - Verify computed `MediaHash` matches the database `MediaHash` (case-insensitive hexadecimal comparison)
- **FR-009**: The tool MUST classify every evaluated file into exactly one category:
  - `Recovered`: Valid DICOM matching database records
  - `Corrupt`: Unparseable header, premature EOF, corrupted pixel stream, or decoding failure
  - `Duplicate`: Identical `MediaHash` and metadata to an already sorted file
  - `Unmatched`: Valid, readable DICOM file with no matching database record in the provided CSV tables
- **FR-010**: Recovered files MUST be placed in `<output_dir>/<InternalCardId>/` named according to the template:
  - Multi-layer: `Volume_<FormattedDateTime>.dcm`
  - Single-layer: `RasterImage_<FormattedDateTime>.dcm`
  - Where `<FormattedDateTime>` is formatted as `YYYY-MM-DD_HH-mm-ss` in 24-hour time.
  - Destination files MUST NEVER be overwritten; collision detection MUST guarantee uniqueness before any move occurs.
- **FR-011**: Files classified as `Corrupt` MUST be moved/copied to `<output_dir>/corrupt/`.
- **FR-012**: Files classified as `Duplicate` MUST be safely segregated into `<output_dir>/duplicates/` without overwriting prior files.
- **FR-013**: Files classified as `Unmatched` MUST be moved/copied to `<output_dir>/unmatched/`.
- **FR-014**: The CLI MUST display an interactive, real-time terminal progress bar indicating current file progress, processing rate (files/second), and elapsed time. On resumed runs, the progress bar MUST display cumulative progress reflecting already processed items.
- **FR-015**: Upon completion, the tool MUST print a comprehensive summary report directly to the terminal (stdout) and write both `recovery_report.txt` and `recovery_report.json` to `<output_dir>`, detailing:
  - Total files scanned from input directory
  - Count and percentage of successfully recovered files
  - Count of corrupt files
  - Count of duplicate files
  - Count of unmatched files
  - Total distinct `RootNode` media files in `MediaBase.csv`
  - Total distinct `RootNode` media files recovered
  - Overall MediaBase recovery rate (`Recovered distinct RootNodes / Total distinct RootNodes in MediaBase * 100%`), explicitly accounting for files lost in the prior file carving stage
  - Total execution duration and average throughput
  - Detailed per-file log including original path, target path, status, and reason for quarantine (if applicable).
- **FR-016**: The tool MUST operate with full behavioral equivalence and path safety on both Linux and Windows operating systems.
- **FR-017**: The tool MUST produce standardized, deterministic exit codes:
  - `0`: Success (all files processed; recovery completed)
  - `1`: User/CLI argument or configuration error
  - `2`: Input CSV validation error (missing columns or unreadable files)
  - `3`: Filesystem access or I/O failure
  - `4`: Process interrupted by user signal (SIGINT / SIGTERM)
- **FR-018**: During file processing, the tool MUST record an atomic transaction journal logging the source path, destination path, hash, and status of every moved or copied file within the local state database.
- **FR-019**: When invoked with `--undo <output_dir>`, the tool MUST query the transaction journal from the local state database and restore all moved files to their exact pre-sorting source paths, ensuring complete rollback fidelity.
- **FR-020**: The tool MUST initialize and maintain a lightweight, embedded local database (stored at `<output_dir>/.dcmsiv_state.db`) to track all scan progress, file states, extracted metadata, and operation transactions.
- **FR-021**: File state transitions (`Pending`, `Hashed`, `Matched`, `Moved`, `Quarantined`) MUST be written to the local state database using atomic, crash-resilient transactions (WAL mode).
- **FR-022**: The tool MUST handle termination signals (SIGINT, SIGTERM) cleanly by finishing in-flight transactions, closing database connections, and exiting gracefully.
- **FR-023**: When re-executed on an existing output directory, the tool MUST detect `.dcmsiv_state.db`, verify previously processed files, and resume execution by skipping already sorted, corrupt, duplicate, or unmatched files.

### Key Entities *(include if feature involves data)*

- **Patient Record**:
  - `PatientId`: Integer/String identifier internal to a specific SIDEXIS server instance.
  - `InternalCardId`: Clinic-wide unique patient identifier (chart number) recognized across all servers, corresponding to the primary patient directory name.
- **MediaBase Record**:
  - `PatientId`: Foreign key linking to the `Patient Record`.
  - `CreationDate`: Timestamp corresponding to when the image acquisition took place.
  - `MediaHash`: Pre-calculated SHA-1 hexadecimal checksum representing the middle layer (for volumes) or single layer (for raster scans, using Windows DIB memory layout with BGR channel order and 4-byte row alignment for 8-bit RGB).
  - `RootNode`: Unifying identifier grouping multiple database rows that belong to the same physical media file.
- **DICOM Scan Asset**:
  - `SourcePath`: File path where the unsorted scan was located.
  - `PatientIdentifier`: Extracted `PatientID` or fallback `RETIRED_OtherPatientIDs`.
  - `AcquisitionTimestamp`: Normalized acquisition date/time.
  - `LayerCount`: Number of image frames/layers (1 for 2D raster, >1 for 3D CBCT volume).
  - `ComputedHash`: SHA-1 hash calculated from the relevant pixel layer.
  - `Disposition`: Final status classification (`Recovered`, `Corrupt`, `Duplicate`, `Unmatched`).
  - `DestinationPath`: Target file path where the asset was placed.
- **Local State Database (`.dcmsiv_state.db`)**:
  - `scanned_files`: Tracks file paths, size, mtime, processing status, and error messages.
  - `dicom_metadata`: Cached metadata (tags, timestamps, frame counts) to avoid re-reading headers.
  - `media_hashes`: Cached layer hashes to avoid re-computing SHA-1 checksums on resume.
  - `transactions`: Journal records of every move/copy operation for audit and `--undo` rollbacks.
- **Recovery Audit Summary**:
  - Aggregated counts, execution metrics, distinct `RootNode` completeness statistics, and itemized audit records.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of uncorrupted DICOM files with corresponding database entries in the provided CSV tables are correctly reconciled and placed in the appropriate patient folder.
- **SC-002**: Zero data loss: Under no circumstances is any source or destination file overwritten, silently skipped, or lost during move operations; collision detection guarantees 0 overwritten files.
- **SC-003**: 100% of unreadable, truncated, or broken DICOM files are isolated in the `corrupt/` folder with documented failure reasons in the recovery log.
- **SC-004**: Processing throughput achieves at least 50 single-layer scans per second and at least 15 volumetric scans per second on standard SSD storage.
- **SC-005**: Memory consumption remains strictly bounded below 500 MB at all times, regardless of whether processing 100 or 100,000 files.
- **SC-006**: Generated recovery reports (both the terminal stdout display and persistent `recovery_report.json`/`recovery_report.txt`) achieve 100% numerical reconciliation with the physical file counts across output directories (`<InternalCardId>/`, `corrupt/`, `duplicates/`, and `unmatched/`).
- **SC-007**: 100% test suite and operational pass rate across both Linux and Windows platforms.
- **SC-008**: 100% rollback fidelity: Running `--undo` on a completed move run restores all moved files back to their exact original source directory structure without data corruption or loss.
- **SC-009**: Database completeness accuracy: The report accurately calculates and displays the distinct `RootNode` recovery percentage against total distinct `RootNode` entries in `MediaBase.csv` with 100% mathematical precision.
- **SC-010**: Instantaneous resumption: Upon restart following interruption or failure, `dcmsiv` queries the local state database and resumes active processing of remaining files within 3 seconds, without re-reading or re-hashing completed files.
- **SC-011**: Zero duplication upon resume: Resuming after interruption yields the exact same final directory structure, file counts, and report as an uninterrupted run with 0 duplicate moves or overwritten files.
- **SC-012**: Crash resilience: Hard termination (SIGKILL or simulated sudden crash) leaves the state database uncorrupted, and subsequent execution resumes seamlessly without data loss.

## Assumptions

- The input database tables are provided as clean CSV files exported from SIDEXIS Microsoft SQL Server (`Patient` table and `MediaBase` table).
- The default operational mode is `move` to organize files in place and clean up the unorganized recovery staging folder; an optional `--copy` flag is available if non-destructive replication is preferred.
- Destination collision detection is mandatory: no file move may overwrite an existing target file.
- The `RootNode` column in `MediaBase.csv` serves as the primary grouping key for deduplicating multiple database entries referring to a single physical DICOM scan file.
- Total expected original files is defined as `COUNT(DISTINCT RootNode)` in `MediaBase.csv`.
- The state database is created at `<output_dir>/.dcmsiv_state.db` using embedded crash-safe storage (e.g. SQLite with WAL mode).
- The state database file (`.dcmsiv_state.db`) is strictly local to the runtime machine, excluded from git tracking per Principle I of the project constitution (`*.db` in `.gitignore`), and never committed or shared.
- The patient's primary clinic-facing directory name is `InternalCardId` (clinic chart number), as this is unique across all SIDEXIS servers and matches what clinical staff use to identify patients.
- Single-layer DICOM scans have a frame count of 1 (or lack a `NumberOfFrames` attribute), while multi-layer volumetric scans have a frame count > 1.
- Middle layer for multi-layer scans is 0-indexed at frame $\lfloor \text{NumberOfFrames} / 2 \rfloor$.
- SHA-1 hash is computed over the pixel byte buffer of the target frame: for 16-bit monochrome, directly over the raw bytes as stored; for 8-bit RGB single-layer images, over a reconstructed Windows DIB memory layout (BGR channel order, rows padded to 4-byte DWORD boundaries, top-to-bottom); matching the hashing algorithm utilized by SIDEXIS when storing `MediaHash`.
- All SIDEXIS DICOM pixel data payloads are strictly raw and uncompressed Little Endian rasters or volumes, enabling direct positional seeks to calculate frame offsets and SHA-1 checksums without decompression.
- Date and time formatting in filenames follows the standard `YYYY-MM-DD_HH-mm-ss` format for human readability and chronological filesystem sorting.
- Any PHI encountered during local recovery is protected in accordance with the project constitution and never committed or transmitted over external networks.
