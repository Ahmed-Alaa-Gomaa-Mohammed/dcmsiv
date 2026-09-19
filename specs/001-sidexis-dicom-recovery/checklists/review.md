# Comprehensive Requirements Quality Checklist: SIDEXIS DICOM Recovery & Sorter (dcmsiv)

**Purpose**: Validate the completeness, clarity, consistency, and testability of requirements for the SIDEXIS DICOM recovery and sorting tool (`dcmsiv`) prior to pull request code review and implementation.  
**Created**: 2026-09-19  
**Feature**: [spec.md](../spec.md) | [plan.md](../plan.md) | [cli-contract.md](../contracts/cli-contract.md)  

**Review Ownership**: This checklist is a reviewer-owned requirements-quality review artifact. Mark an item `[x]` only when the reviewer determines the requirements-quality criterion is satisfied.  
**Marker Semantics**: `[x]` means the criterion has been reviewed and satisfied for requirements quality. It does not mean implementation work is complete.  

---

## 1. Medical Privacy & Data Safety Requirements

- [ ] CHK001 - Are Protected Health Information (PHI) exclusion and sanitization requirements explicitly defined for all runtime paths? [Completeness, Spec §FR-016, Constitution Principle I]
- [ ] CHK002 - Does the specification explicitly prohibit the storage or leakage of unmasked patient demographic data in diagnostic and error log streams? [Coverage, Spec §FR-015, Constitution Principle I]
- [ ] CHK003 - Are repository ignore rules explicitly defined to prevent local SQLite state databases (`.dcmsiv_state.db`) and clinical caches from entering version control? [Consistency, Spec §Assumptions, Plan §Technical Context]
- [ ] CHK004 - Is the zero-overwrite guarantee defined with explicit collision-handling behaviors for both identical and distinct conflicting files? [Clarity, Spec §FR-010, Edge Cases]

## 2. DICOM Parsing & SIDEXIS Database Mapping Requirements

- [ ] CHK005 - Are the exact tag precedence and fallback rules defined when `PatientID` (0010,0020) is absent or whitespace-only? [Clarity, Spec §FR-004, FR-005]
- [ ] CHK006 - Is the middle-layer frame index formula explicitly quantified for both even and odd volumetric frame counts? [Clarity, Spec §FR-007, Edge Cases]
- [ ] CHK007 - Are date/time extraction fallback requirements complete when `AcquisitionDateTime` (0008,002A) is missing or unparseable? [Completeness, Spec §Edge Cases]
- [ ] CHK008 - Is the `RootNode` grouping requirement defined to prevent skewed recovery rate statistics when multiple database entries exist per file? [Consistency, Spec §FR-003, FR-015, Data Model §2]
- [ ] CHK009 - Are requirements specified for handling non-standard character encodings or illegal filesystem path characters in `InternalCardId`? [Edge Case, Spec §Edge Cases]

## 3. Crash Resilience, State Management & Undo Requirements

- [ ] CHK010 - Are the lifecycle states of a candidate file (`Pending`, `Recovered`, `Corrupt`, `Duplicate`, `Unmatched`) exhaustively defined without overlapping transitions? [Completeness, Data Model §Lifecycle]
- [ ] CHK011 - Are state database transaction commit requirements specified to prevent inconsistent state if process termination occurs mid-move? [Consistency, Spec §FR-021, Plan §Technical Context]
- [ ] CHK012 - Does the specification define how `dcmsiv` reconciles files that were moved on disk but not yet recorded in the database prior to a crash? [Coverage, Spec §Edge Cases, FR-023]
- [ ] CHK013 - Are the conditions and boundaries for executing an `--undo` rollback explicitly defined, including missing or corrupted transaction journals? [Clarity, Spec §FR-018, FR-019, Edge Cases]
- [ ] CHK014 - Is the behavior of `--undo` defined when previously moved files have been externally modified or deleted? [Edge Case, Spec §User Story 5, Acceptance Scenario 2]

## 4. Performance, Concurrency & Resource Bound Requirements

- [ ] CHK015 - Is the memory consumption limit (<500 MB) quantified with specific operational boundaries across archive sizes up to 100,000 files? [Measurability, Spec §SC-005, Plan §Technical Context]
- [ ] CHK016 - Are throughput expectations quantified with measurable metrics for both single-layer and volumetric scans on SSD storage? [Measurability, Spec §SC-004]
- [ ] CHK017 - Are thread-safety and lock-free coordination requirements between parallel scanner workers and the SQLite writer clearly specified? [Clarity, Plan §Technical Context, Research §3]
- [ ] CHK018 - Is the positional read optimization (<2 MB per volumetric scan) specified with exact offset calculation formulas? [Clarity, Research §2, Plan §Technical Context]

## 5. CLI Interface, Observability & Reporting Requirements

- [ ] CHK019 - Are all process exit codes (0, 1, 2, 3, 4) deterministically mapped to specific runtime failure conditions? [Clarity, Spec §FR-017, Contract §Exit Codes]
- [ ] CHK020 - Are the terminal progress bar update behaviors defined for both initial runs and resumed executions? [Completeness, Spec §FR-014, Contract §Output Contract]
- [ ] CHK021 - Is the machine-readable JSON report schema (`recovery_report.json`) formally specified with required fields and types? [Clarity, Contract §Structured JSON Report Contract]
- [ ] CHK022 - Are recovery percentage calculations technology-agnostic and verifiable against distinct `RootNode` counts? [Measurability, Spec §SC-009, FR-015]

## 6. Cross-Platform Compatibility & Portability Requirements

- [ ] CHK023 - Are path separator and drive letter handling requirements defined to ensure full behavioral equivalence across Linux and Windows? [Coverage, Spec §FR-016, Constitution Principle II]
- [ ] CHK024 - Are terminal console rendering requirements specified for environments lacking full ANSI escape code support? [Edge Case, Research §6, Contract §1]

---

## Notes

- Mark items `[x]` only after review confirms the requirement-quality criterion is satisfied.
- Leave items unchecked (`[ ]`) when they still require clarification, correction, or reviewer evaluation.
- `/speckit-implement` reads checklist checkbox state as a quality gate and must not modify markers.
- `checklists/requirements.md` has a separate built-in lifecycle maintained by `/speckit-specify` and `/speckit-clarify`.
- Add comments or findings inline under the relevant items.
- All items are numbered sequentially (CHK001–CHK024) for traceability.
