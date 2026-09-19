mod common;

#[path = "integration/recovery_tests.rs"]
mod recovery_tests;

#[path = "integration/resume_tests.rs"]
mod resume_tests;

#[path = "integration/quarantine_tests.rs"]
mod quarantine_tests;

#[path = "integration/undo_tests.rs"]
mod undo_tests;

#[path = "integration/dry_run_tests.rs"]
mod dry_run_tests;

#[path = "integration/memory_bound_tests.rs"]
mod memory_bound_tests;

#[path = "integration/quickstart_scenarios_test.rs"]
mod quickstart_scenarios_test;
