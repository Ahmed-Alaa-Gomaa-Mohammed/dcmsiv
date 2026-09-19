use crate::dicom::types::{DicomScan, FileDisposition, ProcessingEvent, TransactionRecord};
use chrono::Utc;
use crossbeam_channel::{unbounded, Sender};
use rusqlite::{params, Connection, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub struct StateStore {
    conn: Connection,
}

impl StateStore {
    /// Opens or creates the SQLite state database at the given path
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.init_pragmas()?;
        store.init_schema()?;
        Ok(store)
    }

    /// Sets high-performance crash-resilient PRAGMAs
    fn init_pragmas(&self) -> Result<()> {
        self.conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;",
        )?;
        Ok(())
    }

    /// Creates tables and indices if they do not already exist
    pub fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS scanned_files (
                file_path TEXT PRIMARY KEY,
                file_size INTEGER NOT NULL,
                mtime INTEGER NOT NULL,
                patient_identifier TEXT,
                acquisition_datetime TEXT,
                layer_count INTEGER,
                middle_layer_index INTEGER,
                media_hash TEXT,
                status TEXT NOT NULL,
                target_path TEXT,
                error_message TEXT,
                processed_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_scanned_status ON scanned_files(status);
            CREATE INDEX IF NOT EXISTS idx_scanned_hash ON scanned_files(media_hash);

            CREATE TABLE IF NOT EXISTS transactions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_path TEXT NOT NULL,
                destination_path TEXT NOT NULL,
                operation_type TEXT NOT NULL,
                status TEXT NOT NULL,
                timestamp TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_trans_status ON transactions(status);

            CREATE TABLE IF NOT EXISTS recovered_root_nodes (
                root_node TEXT PRIMARY KEY,
                first_recovered_file TEXT NOT NULL,
                patient_id INTEGER NOT NULL
            );",
        )?;
        Ok(())
    }

    /// Resets all state tables (used by --reset-state)
    pub fn reset_state(&self) -> Result<()> {
        self.conn.execute_batch(
            "DELETE FROM scanned_files;
             DELETE FROM transactions;
             DELETE FROM recovered_root_nodes;",
        )?;
        Ok(())
    }

    /// Checks if a file path has already been processed
    pub fn is_file_processed(&self, path: &str) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT 1 FROM scanned_files WHERE file_path = ?1 LIMIT 1")?;
        Ok(stmt.exists(params![path])?)
    }

    /// Fetches all previously processed source file paths
    pub fn get_processed_files(&self) -> Result<HashSet<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT file_path FROM scanned_files WHERE status != 'pending'")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut set = HashSet::new();
        for r in rows {
            set.insert(r?);
        }
        Ok(set)
    }

    /// Inserts or updates a file scan state
    pub fn record_file_scan(&mut self, scan: &DicomScan) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO scanned_files (
                    file_path, file_size, mtime, patient_identifier,
                    acquisition_datetime, layer_count, middle_layer_index,
                    media_hash, status, target_path, error_message, processed_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                ON CONFLICT(file_path) DO UPDATE SET
                    status = excluded.status,
                    target_path = excluded.target_path,
                    error_message = excluded.error_message,
                    processed_at = excluded.processed_at",
            )?;

            let patient_identifier = scan.metadata.as_ref().and_then(|m| {
                m.patient_id
                    .clone()
                    .or_else(|| m.other_patient_ids.clone())
            });

            let acq_dt = scan
                .metadata
                .as_ref()
                .and_then(|m| m.acquisition_datetime.map(|dt| dt.to_string()));

            let processed_at = Utc::now().to_rfc3339();
            let source_path_str = scan.source_path.to_string_lossy().to_string();
            let target_path_str = scan
                .destination_path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string());

            stmt.execute(params![
                source_path_str,
                scan.file_size as i64,
                scan.mtime,
                patient_identifier,
                acq_dt,
                scan.layer_count.frames() as i64,
                scan.layer_count.middle_layer_index() as i64,
                scan.computed_hash,
                scan.disposition.to_string(),
                target_path_str,
                scan.error_reason,
                processed_at,
            ])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Logs an atomic move or copy transaction
    pub fn record_transaction(
        &mut self,
        source: &Path,
        destination: &Path,
        operation_type: &str,
    ) -> Result<i64> {
        let timestamp = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO transactions (source_path, destination_path, operation_type, status, timestamp)
             VALUES (?1, ?2, ?3, 'committed', ?4)",
            params![
                source.to_string_lossy().to_string(),
                destination.to_string_lossy().to_string(),
                operation_type,
                timestamp,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Records a recovered RootNode if not already recorded
    pub fn record_recovered_root_node(
        &mut self,
        root_node: &str,
        file: &str,
        patient_id: i64,
    ) -> Result<bool> {
        let rows_affected = self.conn.execute(
            "INSERT OR IGNORE INTO recovered_root_nodes (root_node, first_recovered_file, patient_id)
             VALUES (?1, ?2, ?3)",
            params![root_node, file, patient_id],
        )?;
        Ok(rows_affected > 0)
    }

    /// Returns count of distinct recovered RootNodes
    pub fn get_distinct_recovered_root_nodes_count(&self) -> Result<usize> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(DISTINCT root_node) FROM recovered_root_nodes",
            [],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Summarizes count by status
    pub fn get_counts_by_status(&self) -> Result<HashMap<FileDisposition, usize>> {
        let mut stmt = self
            .conn
            .prepare("SELECT status, COUNT(*) FROM scanned_files GROUP BY status")?;
        let rows = stmt.query_map([], |row| {
            let s: String = row.get(0)?;
            let c: i64 = row.get(1)?;
            Ok((s, c as usize))
        })?;

        let mut map = HashMap::new();
        for r in rows {
            let (status_str, count) = r?;
            if let Ok(disp) = status_str.parse::<FileDisposition>() {
                map.insert(disp, count);
            }
        }
        Ok(map)
    }

    /// Fetches all committed transactions in reverse chronological order (for --undo)
    pub fn get_committed_transactions_reverse(&self) -> Result<Vec<TransactionRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source_path, destination_path, operation_type, status, timestamp
             FROM transactions
             WHERE status = 'committed'
             ORDER BY id DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(TransactionRecord {
                id: Some(row.get(0)?),
                source_path: PathBuf::from(row.get::<_, String>(1)?),
                destination_path: PathBuf::from(row.get::<_, String>(2)?),
                operation_type: row.get(3)?,
                status: row.get(4)?,
                timestamp: row.get(5)?,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    /// Marks a transaction as rolled back
    pub fn mark_transaction_rolled_back(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE transactions SET status = 'rolled_back' WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// Resets a scanned file's status back to pending after rollback
    pub fn reset_file_status_to_pending(&self, file_path: &Path) -> Result<()> {
        self.conn.execute(
            "UPDATE scanned_files SET status = 'pending', target_path = NULL WHERE file_path = ?1",
            params![file_path.to_string_lossy().to_string()],
        )?;
        Ok(())
    }
}

/// Dedicated background writer thread coordinating parallel Rayon workers with SQLite
pub struct BackgroundStateWriter {
    sender: Sender<ProcessingEvent>,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<Result<()>>>,
}

impl BackgroundStateWriter {
    pub fn start(db_path: PathBuf) -> Result<Self> {
        let (sender, receiver) = unbounded::<ProcessingEvent>();
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();

        let handle = thread::spawn(move || {
            let mut store = StateStore::open(&db_path)?;
            let mut batch = Vec::new();

            loop {
                // Try receiving events with timeout so we can periodically flush
                while let Ok(event) = receiver.recv_timeout(Duration::from_millis(50)) {
                    batch.push(event);
                    if batch.len() >= 50 {
                        Self::flush_batch(&mut store, &mut batch)?;
                    }
                }

                if !batch.is_empty() {
                    Self::flush_batch(&mut store, &mut batch)?;
                }

                if shutdown_clone.load(Ordering::Relaxed) && receiver.is_empty() {
                    break;
                }
            }

            if !batch.is_empty() {
                Self::flush_batch(&mut store, &mut batch)?;
            }

            Ok(())
        });

        Ok(Self {
            sender,
            shutdown,
            handle: Some(handle),
        })
    }

    pub fn sender(&self) -> Sender<ProcessingEvent> {
        self.sender.clone()
    }

    fn flush_batch(store: &mut StateStore, batch: &mut Vec<ProcessingEvent>) -> Result<()> {
        let tx = store.conn.transaction()?;
        {
            let mut scan_stmt = tx.prepare_cached(
                "INSERT INTO scanned_files (
                    file_path, file_size, mtime, patient_identifier,
                    acquisition_datetime, layer_count, middle_layer_index,
                    media_hash, status, target_path, error_message, processed_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                ON CONFLICT(file_path) DO UPDATE SET
                    status = excluded.status,
                    target_path = excluded.target_path,
                    error_message = excluded.error_message,
                    processed_at = excluded.processed_at",
            )?;

            let mut trans_stmt = tx.prepare_cached(
                "INSERT INTO transactions (source_path, destination_path, operation_type, status, timestamp)
                 VALUES (?1, ?2, ?3, 'committed', ?4)",
            )?;

            let mut root_stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO recovered_root_nodes (root_node, first_recovered_file, patient_id)
                 VALUES (?1, ?2, ?3)",
            )?;

            for event in batch.drain(..) {
                match event {
                    ProcessingEvent::FileProcessed { scan } => {
                        let patient_identifier = scan.metadata.as_ref().and_then(|m| {
                            m.patient_id
                                .clone()
                                .or_else(|| m.other_patient_ids.clone())
                        });
                        let acq_dt = scan
                            .metadata
                            .as_ref()
                            .and_then(|m| m.acquisition_datetime.map(|dt| dt.to_string()));
                        let processed_at = Utc::now().to_rfc3339();
                        let source_path_str = scan.source_path.to_string_lossy().to_string();
                        let target_path_str = scan
                            .destination_path
                            .as_ref()
                            .map(|p| p.to_string_lossy().to_string());

                        scan_stmt.execute(params![
                            source_path_str,
                            scan.file_size as i64,
                            scan.mtime,
                            patient_identifier,
                            acq_dt,
                            scan.layer_count.frames() as i64,
                            scan.layer_count.middle_layer_index() as i64,
                            scan.computed_hash,
                            scan.disposition.to_string(),
                            target_path_str,
                            scan.error_reason,
                            processed_at,
                        ])?;
                    }
                    ProcessingEvent::TransactionCommitted {
                        source_path,
                        destination_path,
                        operation_type,
                    } => {
                        let timestamp = Utc::now().to_rfc3339();
                        trans_stmt.execute(params![
                            source_path.to_string_lossy().to_string(),
                            destination_path.to_string_lossy().to_string(),
                            operation_type,
                            timestamp,
                        ])?;
                    }
                    ProcessingEvent::RootNodeRecovered {
                        root_node,
                        first_recovered_file,
                        patient_id,
                    } => {
                        root_stmt.execute(params![root_node, first_recovered_file, patient_id])?;
                    }
                    ProcessingEvent::Flush => {}
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn stop(mut self) -> Result<()> {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = self.sender.send(ProcessingEvent::Flush);
        if let Some(handle) = self.handle.take() {
            match handle.join() {
                Ok(res) => res,
                Err(_) => Err(rusqlite::Error::ExecuteReturnedResults),
            }
        } else {
            Ok(())
        }
    }
}
