# Technical Research: SIDEXIS DICOM Recovery & Sorter (dcmsiv)

**Feature**: SIDEXIS DICOM Recovery & Sorter  
**Branch**: `001-sidexis-dicom-recovery`  
**Date**: 2026-09-19  

## Technical Decisions & Rationale

### 1. Implementation Language: Rust

- **Decision**: Rust (stable 1.75+, edition 2021).
- **Rationale**:
  - Predictable zero-cost abstractions, memory safety without garbage collection pauses, and native cross-platform binaries (Linux ELF and Windows PE executables) with zero runtime dependencies.
  - Native filesystem access, granular low-level positional seek operations, and deterministic memory consumption strictly under 500 MB when processing 100,000+ files.
  - Aligns with Constitution Principle II (Cross-Platform Parity) and Principle III (High Performance & Speed).
- **Alternatives Considered**:
  - *Python*: Simpler prototyping, but high memory footprint, slower CPU-bound SHA-1 hashing, and GIL bottlenecks in multi-threading.
  - *Go*: Fast and concurrent, but GC pauses during large heap allocations and less ergonomic zero-copy binary layout inspection compared to Rust.
  - *C++*: Maximum raw speed, but lacks modern memory safety guarantees, complex cross-platform build toolchains, and higher defect risk.

---

### 2. High-Throughput Positional Reads & DICOM Hashing

- **Decision**: Custom zero-copy header stream reader paired with positional seeking (`std::io::Seek` / `SeekFrom::Start`) to calculate pixel data middle-layer offsets directly.
- **Rationale**:
  - In recovered medical archives, individual CBCT volumes often exceed 50 MB to 250 MB each (as observed in local testsets). Loading entire files into memory just to hash the middle layer creates massive I/O saturation and memory bloat.
  - DICOM Part 10 files place pixel data in tag `(7FE0, 0010)`. By parsing only the header elements up to `PixelData`, extracting dimensions (`Rows`, `Columns`, `BitsAllocated`, `SamplesPerPixel`, and `NumberOfFrames`), the tool calculates the exact byte offset of the target middle frame:
    $$\text{FrameSize} = \text{Rows} \times \text{Columns} \times \left(\frac{\text{BitsAllocated}}{8}\right) \times \text{SamplesPerPixel}$$
    $$\text{MiddleIndex} = \lfloor \text{NumberOfFrames} / 2 \rfloor$$
    $$\text{FrameOffset} = \text{PixelDataStartOffset} + (\text{MiddleIndex} \times \text{FrameSize})$$
  - The reader then issues a positional seek directly to `FrameOffset` and streams exactly `FrameSize` bytes (typically 1–2 MB) through the SHA-1 hasher.
  - This turns a 250 MB disk read into a ~64 KB header read + ~1 MB frame read, achieving a >95% reduction in disk I/O per volumetric scan.
  - **Single-Layer Layout-Specific Hashing**:
    - **16-bit MONOCHROME2**: The Pixel Data element value is hashed directly as stored with no channel or byte modifications.
    - **8-bit RGB Interleaved (`PlanarConfiguration = 0`)**: SIDEXIS reconstructs the decoded pixel array into a **Windows DIB (Device Independent Bitmap) memory layout** before hashing. This involves: (1) swapping each pixel's red and blue channels (`R, G, B` $\rightarrow$ `B, G, R`), (2) padding each row to a **4-byte (DWORD) boundary** with `0x00` bytes (`pad_per_row = (4 - (Columns × 3) % 4) % 4`), (3) rows stored top-to-bottom (no vertical flip). The SHA-1 is computed over the resulting `Rows × (Columns × 3 + pad_per_row)` byte buffer. The DICOM trailing pad byte is **not** included — the hash is derived from the decoded pixel content, not the raw Pixel Data element value.
    - **Previous Pad Byte Sweep Hypothesis (Superseded)**: Earlier analysis suggested that SIDEXIS hashed the raw Pixel Data element value including a trailing pad byte, requiring a 256-pad sweep for carved files. Testing against real MediaBase entries confirmed this was incorrect — the DIB row-stride alignment is the actual algorithm. The pad sweep mechanism is no longer needed.
- **Alternatives Considered**:
  - *Full DICOM parsing libraries (e.g. loading complete pixel dataset)*: Rejected because loading full 250 MB volumes exhausts RAM and throttles throughput below the required 15 volumes/sec threshold.
  - *Memory-mapped files (`mmap`)*: Viable on 64-bit OS, but susceptible to SIGBUS on truncated files and problematic on network-mounted Windows SMB shares. Explicit buffered seeking provides safer, verifiable error handling.

---

### 3. Concurrency Architecture: Rayon Worker Pool + Channel-Based SQLite Writer

- **Decision**: Multi-threaded data parallelism using `rayon` for scanning, header extraction, hashing, and classification, combined with an asynchronous MPSC channel to a dedicated SQLite transaction writer thread.
- **Rationale**:
  - CPU/IO intensive tasks (directory walking, binary header inspection, SHA-1 checksumming) scale linearly across CPU cores with `rayon`.
  - SQLite is single-writer in concurrent environments; having multiple worker threads contend for database write locks causes `SQLITE_BUSY` errors and lock contention.
  - Using a bounded crossbeam/mpsc channel allows workers to push `ProcessingEvent`s without blocking. A dedicated background thread drains the channel, batches database operations into atomic transactions (500–1000 items or every 100ms), and guarantees crash-safe write operations with minimal lock overhead.
- **Alternatives Considered**:
  - *Thread-per-file*: Too much thread spawning overhead; exhausted OS thread handles on large archives.
  - *Connection pooling (e.g., `r2d2_sqlite`)*: Workers still contend on SQLite's exclusive database write lock; the single-writer channel pattern completely eliminates write contention.

---

### 4. Local State Database: Embedded SQLite in WAL Mode

- **Decision**: Embedded SQLite (`rusqlite` with bundled SQLite 3) configured with Write-Ahead Logging (`PRAGMA journal_mode=WAL;`) and `PRAGMA synchronous=NORMAL;`.
- **Rationale**:
  - Zero external server setup; lives in a single local file at `<output_dir>/.dcmsiv_state.db`.
  - Fully crash-resilient: WAL mode provides ACID transaction guarantees, ensuring that abrupt termination (SIGINT, power loss, process crash) leaves the database in a consistent state.
  - Complies with Constitution Principle I: excluded from git by `.gitignore` (`*.db`, `*.sqlite`).
  - High performance: can easily absorb tens of thousands of inserts per second in batched transactions.
  - Enables instant query-based resumption: `SELECT file_path FROM scanned_files WHERE status IN ('recovered', 'corrupt', 'duplicate', 'unmatched')`.
- **Alternatives Considered**:
  - *Flat JSON / JSONL state file*: Prone to partial corruption on crash, high serialization overhead on re-reading 100,000 records on resume.
  - *DuckDB / RocksDB*: Heavier dependencies and binary size; SQLite is ubiquitous, battle-tested, lightweight, and natively supported across Linux and Windows.

---

### 5. Collision Prevention & Non-Destructive File Movement

- **Decision**: Two-phase move execution: pre-flight collision check followed by atomic rename / fallback copy-delete with rollback logging.
- **Rationale**:
  - Satisfies the zero-overwrite guarantee (FR-010, SC-002).
  - If destination path exists:
    - If hash matches: classified as `Duplicate`, moved to `<output>/duplicates/` with unique suffix.
    - If hash differs: collision counter `_<counter>` appended (e.g. `Volume_2023-05-12_14-30-22_01.dcm`).
  - On the same filesystem volume, `std::fs::rename` is atomic and near-instantaneous. Across filesystem boundaries (e.g., across drives), fallback to stream copy, hash verify, and source removal ensures data integrity.
- **Alternatives Considered**:
  - *Blind move with overwrite*: Rejected as it violates Constitution Principle IV and causes irrecoverable clinical data loss.

---

### 6. Terminal Progress & Reporting: Indicatif + Console Stdout

- **Decision**: `indicatif` progress bar with formatted throughput (files/sec) and ETA, transitioning upon completion to a clean ANSI/Windows-compatible summary table on stdout.
- **Rationale**:
  - `indicatif` handles multi-threaded tick updates cleanly across Linux ANSI terminals and Windows virtual terminal processing consoles.
  - Upon task completion, the progress bar finishes and the report prints to stdout, followed by persistent generation of `recovery_report.txt` and `recovery_report.json`.
- **Alternatives Considered**:
  - *Periodic log messages*: Clutters terminal and provides poor user experience for interactive CLI users.
