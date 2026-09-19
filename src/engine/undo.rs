use crate::db::state_store::StateStore;
use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug)]
pub enum UndoError {
    DatabaseNotFound(String),
    DatabaseError(rusqlite::Error),
    Io(io::Error),
    MissingDestination(String),
}

impl std::fmt::Display for UndoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UndoError::DatabaseNotFound(p) => {
                write!(f, "State database not found at {p}. Nothing to undo.")
            }
            UndoError::DatabaseError(e) => write!(f, "Database error during undo: {e}"),
            UndoError::Io(e) => write!(f, "Filesystem error during undo: {e}"),
            UndoError::MissingDestination(p) => {
                write!(f, "Destination file not found to restore: {p}")
            }
        }
    }
}

impl std::error::Error for UndoError {}

impl From<rusqlite::Error> for UndoError {
    fn from(e: rusqlite::Error) -> Self {
        UndoError::DatabaseError(e)
    }
}

impl From<io::Error> for UndoError {
    fn from(e: io::Error) -> Self {
        UndoError::Io(e)
    }
}

/// Rolls back committed file move operations recorded in the output directory's state database
pub fn execute_undo(output_dir: &Path) -> Result<usize, UndoError> {
    let db_path = output_dir.join(".dcmsiv_state.db");
    if !db_path.exists() {
        return Err(UndoError::DatabaseNotFound(db_path.display().to_string()));
    }

    let store = StateStore::open(&db_path)?;
    let transactions = store.get_committed_transactions_reverse()?;

    let mut restored_count = 0;

    for tx in transactions {
        if tx.operation_type != "move" {
            continue;
        }

        let id = tx.id.unwrap_or(0);
        let dest = &tx.destination_path;
        let src = &tx.source_path;

        if !dest.exists() {
            eprintln!("Warning: Destination file missing during undo: {}", dest.display());
            continue;
        }

        if let Some(src_parent) = src.parent() {
            fs::create_dir_all(src_parent)?;
        }

        // Restore file to source path
        match fs::rename(dest, src) {
            Ok(()) => {}
            Err(e) => {
                if e.kind() == io::ErrorKind::CrossesDevices || e.raw_os_error() == Some(18) {
                    fs::copy(dest, src)?;
                    fs::remove_file(dest)?;
                } else {
                    return Err(UndoError::Io(e));
                }
            }
        }

        store.mark_transaction_rolled_back(id)?;
        store.reset_file_status_to_pending(src)?;
        restored_count += 1;
    }

    // Attempt clean up of any empty directories inside output_dir
    cleanup_empty_dirs(output_dir);

    Ok(restored_count)
}

/// Recursively removes empty subdirectories (ignoring state db)
fn cleanup_empty_dirs(dir: &Path) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                cleanup_empty_dirs(&path);
                let _ = fs::remove_dir(&path);
            }
        }
    }
}
