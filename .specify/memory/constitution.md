# dcmsiv Constitution

## Core Principles

### I. Zero Sensitive Data & PHI Leakage (NON-NEGOTIABLE)
Under no circumstances shall Protected Health Information (PHI), patient metadata, clinical datasets (such as raw DICOM files containing patient demographics), or production/local databases be committed, staged, or pushed to any git repository or tracking branch.
- Repository hygiene MUST be strictly enforced: repository ignore rules MUST exclude all DICOM files (`*.dcm`, `*.dicom`), test sets, database files (`*.db`, `*.sqlite`, `*.parquet`), and local diagnostic caches.
- Automated testing and CI/CD pipelines MUST only use synthetic, de-identified, or deterministically generated mock DICOM objects that contain no identifiable patient attributes.
- Local test sets containing clinical or real-world data MUST remain strictly untracked on local machines and MUST never be packaged or distributed with source releases.
- **Rationale**: DICOM files routinely carry sensitive medical records protected by legal and regulatory frameworks (e.g., HIPAA, GDPR). Leakage of patient data creates severe ethical violations, legal liability, and regulatory sanctions.

### II. Cross-Platform Parity (Linux & Windows)
The `dcmsiv` CLI tool MUST provide identical capabilities, sorting behavior, validation outcomes, and command-line semantics across both Linux and Windows environments.
- All file system operations, directory traversals, path representations, and path separators MUST be platform-agnostic and resilient to differences in case sensitivity, path length limits, and permission models.
- Build configurations, tests, and release pipelines MUST treat Linux and Windows as first-class target environments with equal test coverage and verification gates.
- Output formatting and terminal interactions MUST render consistently across POSIX shells and Windows consoles (e.g., PowerShell, Command Prompt, Windows Terminal).
- **Rationale**: Clinical, hospital, and research IT infrastructures frequently deploy mixed environments (such as Linux-based PACS/archive servers alongside Windows clinical workstations); cross-platform consistency is essential for seamless operation.

### III. High Performance & Speed
`dcmsiv` MUST be engineered for high-throughput execution when scanning, sorting, and validating large-scale DICOM image repositories.
- DICOM header inspection MUST avoid unnecessary disk I/O or full-file decoding: parsers MUST read only the required metadata tags and file meta headers without loading full pixel data payloads into memory unless explicitly requested.
- Sorting and validation workflows MUST employ efficient concurrent or multi-threaded processing where available, while bounding memory usage to ensure deterministic resource consumption regardless of dataset scale.
- Memory allocation MUST be bounded and predictable, avoiding full-dataset buffering or memory leaks during long-running batch operations.
- **Rationale**: Medical imaging datasets frequently encompass hundreds of gigabytes, thousands of series, and millions of slices; processing speed and resource efficiency are critical to operational viability.

### IV. Uncompromising Correctness & DICOM Integrity
Every sorting operation, file transformation, and validation evaluation MUST prioritize data integrity and strict adherence to the DICOM standard (NEMA PS3 / ISO 12052).
- Validation MUST rigorously verify DICOM Part 10 compliance, including preamble, DICM prefix, File Meta Information headers, Transfer Syntaxes, and mandatory SOP Class UIDs.
- Sorting and file destination path construction MUST be deterministic, idempotent, and collision-resistant. Repeated executions over identical datasets MUST yield identical structural outputs.
- Destructive operations (moving, renaming, or modifying files) MUST never silently drop, truncate, or overwrite files without explicit user instruction and verified collision resolution strategies.
- **Rationale**: In clinical imaging, misplaced, corrupted, or misidentified DICOM instances can disrupt the imaging hierarchy (Patient -> Study -> Series -> Instance) and lead to catastrophic diagnostic errors or irrecoverable data loss.

### V. Explicit & Unambiguous Reporting
The CLI tool MUST provide transparent, actionable, and deterministic reporting of all operations, explicitly detailing both successes and failures.
- Silent failures, swallowed errors, and unhandled exceptions are strictly prohibited; every skipped file, corrupt header, validation error, or I/O failure MUST be recorded and communicated.
- The CLI MUST use standardized, deterministic process exit codes (exit code 0 for unconditional success, distinct non-zero exit codes mapped to specific failure categories such as validation failures, I/O errors, or invalid arguments).
- Reporting MUST support dual modalities: clean human-readable summaries (progress indicators, clear error diagnostics on standard streams) and structured, machine-parsable formats (such as JSON) to facilitate integration into automated ingestion pipelines.
- **Rationale**: Unattended ingestion pipelines and clinical workflow operators require dependable status reporting to isolate corrupted files, audit pipeline integrity, and prevent cascading pipeline failures.

## Security, Privacy & Data Compliance Standards
1. **Local-First Processing**: `dcmsiv` operates strictly on local or mounted network filesystems; it MUST NOT transmit DICOM metadata, payloads, or telemetry over external networks.
2. **Leakage Prevention**: All developers and CI systems MUST implement automated pre-commit scanning or linting to block any accidental staging of `.dcm`, `.dicom`, or database artifacts.
3. **Audit Trail**: Validation failures and sorting actions SHOULD produce structured audit logs detailing file paths, UIDs, and error descriptions without logging unmasked sensitive demographic data to insecure locations.

## Quality Gates & Verification Standards
1. **Cross-Platform Test Execution**: Automated CI workflows MUST execute all unit and integration test suites on both Linux and Windows environments before any pull request is merged.
2. **Deterministic Test Fixtures**: All committed test fixtures MUST consist exclusively of synthetic or fully anonymized public-domain DICOM files with zero PHI.
3. **Failure-Case Coverage**: Test suites MUST validate negative scenarios (corrupted preambles, truncated files, invalid VRs, missing mandatory tags, and filesystem collision edge cases) to ensure explicit error reporting and proper exit codes.
4. **Performance Benchmarking**: Regression testing MUST monitor parsing and sorting throughput against standard synthetic test benchmarks to detect performance regressions early.

## Governance
This constitution forms the primary governing agreement for the `dcmsiv` codebase, superseding conflicting informal guidelines or ad-hoc practices.
- **Compliance**: All pull requests, feature specifications, architectural decisions, and code implementations MUST verify compliance with the core principles defined herein.
- **Amendment Process**: Amendments to this constitution require a documented proposal detailing the motivation, impact assessment, and migration strategy, followed by formal approval.
- **Versioning Policy**: This constitution follows Semantic Versioning:
  - **MAJOR** version increments: Substantive revisions, removals, or fundamental redefinitions of core principles (e.g., changes to privacy rules or platform commitments).
  - **MINOR** version increments: Addition of new principles, sections, or materially expanded compliance requirements.
  - **PATCH** version increments: Clarifications, wording refinements, formatting improvements, or non-semantic corrections.

**Version**: 1.0.0 | **Ratified**: 2026-09-19 | **Last Amended**: 2026-09-19
