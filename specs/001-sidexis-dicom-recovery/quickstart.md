# Quickstart Validation Guide: dcmsiv

**Feature**: SIDEXIS DICOM Recovery & Sorter  
**Branch**: `001-sidexis-dicom-recovery`  
**Date**: 2026-09-19  

This guide provides runnable end-to-end validation scenarios for `dcmsiv` using synthetic fixtures.

## Prerequisites

- Rust 1.75+ and Cargo installed.
- Target platform: Linux (x86_64 / aarch64) or Windows (x86_64).
- Built binary: `cargo build --release` producing target executable `target/release/dcmsiv` (or `target\release\dcmsiv.exe` on Windows).

---

## Scenario 1: Dry-Run Preview (Zero Disk Changes)

Validate that `--dry-run` accurately evaluates all files, verifies against database tables, and produces a complete forecast without modifying files on disk.

```bash
# 1. Run simulation
dcmsiv \
  --input tests/fixtures/unsorted \
  --output tests/fixtures/sorted \
  --patient-csv tests/fixtures/Patient.csv \
  --mediabase-csv tests/fixtures/MediaBase.csv \
  --dry-run

# 2. Verify: Output directory remains untouched
test ! -d tests/fixtures/sorted/corrupt
```

**Expected Outcome**:
- Progress bar completes.
- Terminal prints simulated recovery metrics.
- No files are created, moved, or altered.

---

## Scenario 2: Standard Recovery & Sorting (Move Mode)

Validate that single-layer scans are named `RasterImage_<DateTime>.dcm`, multi-layer scans are named `Volume_<DateTime>.dcm`, and all are placed inside `<output>/<InternalCardId>/`.

```bash
# 1. Execute recovery
dcmsiv \
  --input tests/fixtures/unsorted \
  --output tests/fixtures/sorted \
  --patient-csv tests/fixtures/Patient.csv \
  --mediabase-csv tests/fixtures/MediaBase.csv

# 2. Verify sorted file hierarchy
ls -la tests/fixtures/sorted/CARD_1001/RasterImage_2023-05-12_14-30-22.dcm
ls -la tests/fixtures/sorted/CARD_1002/Volume_2023-06-15_09-15-00.dcm

# 3. Verify state database and reports generated
test -f tests/fixtures/sorted/.dcmsiv_state.db
test -f tests/fixtures/sorted/recovery_report.json
test -f tests/fixtures/sorted/recovery_report.txt
```

**Expected Outcome**:
- Clean patient directory structure.
- State database initialized in WAL mode.
- Report confirms 100% reconciliation.

---

## Scenario 3: Interruption & Resume Validation

Validate that if `dcmsiv` is stopped midway, re-running the command seamlessly resumes without duplicating work or corrupting state.

```bash
# 1. Start recovery in background and interrupt it after 1 second
dcmsiv \
  --input tests/fixtures/large_batch \
  --output tests/fixtures/sorted_resume \
  --patient-csv tests/fixtures/Patient.csv \
  --mediabase-csv tests/fixtures/MediaBase.csv &
PID=$!
sleep 1
kill -INT $PID

# 2. Re-run identical command
dcmsiv \
  --input tests/fixtures/large_batch \
  --output tests/fixtures/sorted_resume \
  --patient-csv tests/fixtures/Patient.csv \
  --mediabase-csv tests/fixtures/MediaBase.csv
```

**Expected Outcome**:
- On resume, the tool prints: `Resuming from .dcmsiv_state.db (X files already completed)`.
- Previously moved files are skipped without re-reading or re-hashing.
- Final summary report reflects the complete combined batch.

---

## Scenario 4: Rollback via `--undo`

Validate that `--undo` returns all moved files back to their exact original locations.

```bash
# 1. Run undo on the sorted destination
dcmsiv --undo tests/fixtures/sorted

# 2. Verify all files returned to unsorted source folder
test -f tests/fixtures/unsorted/01.dcm
test -f tests/fixtures/unsorted/02.dcm

# 3. Verify destination directories are cleared
test ! -d tests/fixtures/sorted/CARD_1001
```

**Expected Outcome**:
- All moved files safely returned to their original input directory.
- Transaction journal records marked `rolled_back`.
- Zero file corruption or loss.

---

## Scenario 5: Quarantine of Corrupt & Duplicate Scans

Validate that broken DICOM headers go to `corrupt/` and duplicate scans go to `duplicates/`.

```bash
# Verify isolated folders
test -f tests/fixtures/sorted/corrupt/truncated_file.dcm
test -f tests/fixtures/sorted/duplicates/duplicate_scan_01.dcm
test -f tests/fixtures/sorted/unmatched/unknown_patient.dcm
```

**Expected Outcome**:
- Damaged and duplicate files are quarantined safely without contaminating patient directories.
