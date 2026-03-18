// Copyright (c) 2026 Tyler Martin
// Licensed under FSL-1.1-ALv2 (see LICENSE)

use crate::errors::{Result, StoreError};
use rusqlite::Connection;
use std::path::PathBuf;
use tracing::info;

pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open or create the database at the default location.
    /// Default: ~/.config/envelope-email/envelope.db
    pub fn open_default() -> Result<Self> {
        let path = Self::default_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                StoreError::Config(format!("cannot create config dir: {e}"))
            })?;
        }
        let db = Self::open(&path)?;

        // Restrict database file to owner-only access
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&path, perms).map_err(|e| {
                StoreError::Config(format!("cannot set database permissions: {e}"))
            })?;
        }

        Ok(db)
    }

    /// Open or create the database at a specific path.
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    /// Open an in-memory database (for testing).
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    fn default_path() -> PathBuf {
        let config_dir = dirs_next::config_dir()
            .unwrap_or_else(|| PathBuf::from(".config"));
        config_dir.join("envelope-email").join("envelope.db")
    }

    fn migrate(&self) -> Result<()> {
        info!("running database migrations");
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS accounts (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                username TEXT NOT NULL UNIQUE,
                domain TEXT NOT NULL,
                smtp_host TEXT NOT NULL,
                smtp_port INTEGER NOT NULL DEFAULT 587,
                imap_host TEXT NOT NULL,
                imap_port INTEGER NOT NULL DEFAULT 993,
                smtp_username TEXT,
                imap_username TEXT,
                display_name TEXT,
                encrypted_password TEXT NOT NULL,
                encrypted_smtp_password TEXT,
                encrypted_imap_password TEXT,
                signature_text TEXT,
                signature_html TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS drafts (
                id TEXT PRIMARY KEY,
                account_id TEXT NOT NULL REFERENCES accounts(id),
                status TEXT NOT NULL DEFAULT 'draft',
                to_addr TEXT NOT NULL,
                cc_addr TEXT,
                bcc_addr TEXT,
                reply_to TEXT,
                subject TEXT,
                text_content TEXT,
                html_content TEXT,
                in_reply_to TEXT,
                metadata TEXT,
                attachments TEXT NOT NULL DEFAULT '[]',
                message_id TEXT,
                send_after TEXT,
                snoozed_until TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                sent_at TEXT,
                created_by TEXT
            );

            CREATE TABLE IF NOT EXISTS action_log (
                id TEXT PRIMARY KEY,
                account_id TEXT NOT NULL,
                action_type TEXT NOT NULL,
                confidence REAL NOT NULL DEFAULT 0.0,
                justification TEXT NOT NULL DEFAULT '',
                action_taken TEXT NOT NULL DEFAULT '',
                message_id TEXT,
                draft_id TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS license_keys (
                id TEXT PRIMARY KEY,
                token TEXT NOT NULL,
                licensee TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                features TEXT NOT NULL DEFAULT '[]',
                activated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_drafts_account_status
                ON drafts(account_id, status);
            CREATE INDEX IF NOT EXISTS idx_drafts_send_after
                ON drafts(send_after) WHERE status = 'draft';
            CREATE INDEX IF NOT EXISTS idx_action_log_account
                ON action_log(account_id, created_at);
            ",
        )?;
        Ok(())
    }
}
