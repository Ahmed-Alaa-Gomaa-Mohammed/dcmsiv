# dcmsiv

**dcmsiv** is a high-performance DICOM sorter and validator CLI tool designed for speed, correctness, and cross-platform reliability across Linux and Windows.

## Core Pillars & Principles

- **Zero Sensitive Data & PHI Leakage (NON-NEGOTIABLE)**: Protected Health Information (PHI), clinical datasets, and patient databases are strictly prohibited from git tracking. All automated testing uses synthetic or de-identified data.
- **Cross-Platform Parity**: Full operational equivalence and identical behavior across Linux and Windows environments.
- **High Performance & Speed**: High-throughput sorting and validation through lazy header parsing, bounded memory consumption, and parallelized execution.
- **Uncompromising Correctness**: Strict DICOM Part 10 standards compliance, collision resistance, and deterministic, idempotent file sorting.
- **Explicit Reporting**: Transparent operational feedback with structured (JSON) and human-readable output, standard exit codes, and no silent failures.

For governance rules, compliance standards, and architectural guidelines, see the [Project Constitution](.specify/memory/constitution.md).
