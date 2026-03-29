// Copyright (c) 2026 Tyler Martin
// Licensed under FSL-1.1-ALv2 (see LICENSE)

//! IMAP folder auto-detection.
//!
//! Detects well-known folder names (Drafts, Sent, etc.) across different
//! email providers and caches the results per account.

use envelope_email_store::Database;
use tracing::{debug, info, warn};

use crate::errors::ImapError;
use crate::imap;

/// Common drafts folder names across email providers, tried in priority order.
const DRAFTS_FOLDER_CANDIDATES: &[&str] = &[
    "Drafts",
    "[Gmail]/Drafts",
    "Draft",
    "INBOX.Drafts",
    "INBOX/Drafts",
];

/// Common sent folder names across email providers, tried in priority order.
const SENT_FOLDER_CANDIDATES: &[&str] = &[
    "Sent",
    "Sent Mail",
    "Sent Messages",
    "Sent Items",
    "[Gmail]/Sent Mail",
    "INBOX.Sent",
    "INBOX.Sent Messages",
];

/// Detect the drafts folder for an account by checking IMAP folder list.
///
/// Tries common drafts folder names in priority order, then falls back to
/// case-insensitive search. Caches the result in the `detected_folders` table.
pub async fn detect_drafts_folder(
    client: &mut imap::ImapClient,
    db: &Database,
    account_id: &str,
) -> Result<Option<String>, ImapError> {
    // Check cached value first
    if let Ok(Some(cached)) = db.get_drafts_folder(account_id) {
        debug!("using cached drafts folder: {cached}");
        return Ok(Some(cached));
    }

    // List all folders and match against known patterns
    let folders = imap::list_folders(client).await?;
    let folder_set: std::collections::HashSet<&str> = folders.iter().map(|s| s.as_str()).collect();

    for candidate in DRAFTS_FOLDER_CANDIDATES {
        if folder_set.contains(*candidate) {
            info!("detected drafts folder: {candidate}");
            if let Err(e) = db.set_detected_folder(account_id, "drafts", candidate) {
                warn!("failed to cache drafts folder: {e}");
            }
            return Ok(Some(candidate.to_string()));
        }
    }

    // Case-insensitive fallback
    for folder in &folders {
        let lower = folder.to_lowercase();
        if lower.contains("draft") {
            info!("detected drafts folder (fuzzy): {folder}");
            if let Err(e) = db.set_detected_folder(account_id, "drafts", folder) {
                warn!("failed to cache drafts folder: {e}");
            }
            return Ok(Some(folder.clone()));
        }
    }

    warn!("no drafts folder detected for account {account_id}");
    Ok(None)
}

/// Detect the sent folder for an account by checking IMAP folder list.
///
/// Tries common sent folder names in priority order, then falls back to
/// case-insensitive search. Caches the result in the `detected_folders` table.
pub async fn detect_sent_folder(
    client: &mut imap::ImapClient,
    db: &Database,
    account_id: &str,
) -> Result<Option<String>, ImapError> {
    // Check cached value first
    if let Ok(Some(cached)) = db.get_sent_folder(account_id) {
        debug!("using cached sent folder: {cached}");
        return Ok(Some(cached));
    }

    // List all folders and match against known patterns
    let folders = imap::list_folders(client).await?;
    let folder_set: std::collections::HashSet<&str> = folders.iter().map(|s| s.as_str()).collect();

    for candidate in SENT_FOLDER_CANDIDATES {
        if folder_set.contains(*candidate) {
            info!("detected sent folder: {candidate}");
            if let Err(e) = db.set_detected_folder(account_id, "sent", candidate) {
                warn!("failed to cache sent folder: {e}");
            }
            return Ok(Some(candidate.to_string()));
        }
    }

    // Case-insensitive fallback
    for folder in &folders {
        let lower = folder.to_lowercase();
        if lower.contains("sent") && !lower.contains("unsent") {
            info!("detected sent folder (fuzzy): {folder}");
            if let Err(e) = db.set_detected_folder(account_id, "sent", folder) {
                warn!("failed to cache sent folder: {e}");
            }
            return Ok(Some(folder.clone()));
        }
    }

    warn!("no sent folder detected for account {account_id}");
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drafts_folder_candidates_complete() {
        // Verify all expected providers are covered
        let candidates: Vec<&str> = DRAFTS_FOLDER_CANDIDATES.to_vec();
        assert!(candidates.contains(&"Drafts")); // Standard IMAP (Migadu, Fastmail, etc.)
        assert!(candidates.contains(&"[Gmail]/Drafts")); // Gmail
        assert!(candidates.contains(&"Draft")); // Some providers use singular
        assert!(candidates.contains(&"INBOX.Drafts")); // Dovecot with dot separator
        assert!(candidates.contains(&"INBOX/Drafts")); // Dovecot with slash separator
    }

    #[test]
    fn test_sent_folder_candidates_complete() {
        let candidates: Vec<&str> = SENT_FOLDER_CANDIDATES.to_vec();
        assert!(candidates.contains(&"Sent"));
        assert!(candidates.contains(&"Sent Mail"));
        assert!(candidates.contains(&"[Gmail]/Sent Mail"));
        assert!(candidates.contains(&"INBOX.Sent"));
    }

    #[test]
    fn test_drafts_folder_db_cache() {
        let db = Database::open_memory().unwrap();

        // No drafts folder cached yet
        assert!(db.get_drafts_folder("acct1").unwrap().is_none());

        // Cache a standard drafts folder
        db.set_detected_folder("acct1", "drafts", "Drafts").unwrap();
        assert_eq!(
            db.get_drafts_folder("acct1").unwrap().as_deref(),
            Some("Drafts")
        );

        // Gmail-style drafts folder overwrites the cached value
        db.set_detected_folder("acct1", "drafts", "[Gmail]/Drafts")
            .unwrap();
        assert_eq!(
            db.get_drafts_folder("acct1").unwrap().as_deref(),
            Some("[Gmail]/Drafts")
        );

        // Different accounts have independent caches
        assert!(db.get_drafts_folder("acct2").unwrap().is_none());
        db.set_detected_folder("acct2", "drafts", "INBOX.Drafts")
            .unwrap();
        assert_eq!(
            db.get_drafts_folder("acct2").unwrap().as_deref(),
            Some("INBOX.Drafts")
        );
        // acct1 still has Gmail drafts
        assert_eq!(
            db.get_drafts_folder("acct1").unwrap().as_deref(),
            Some("[Gmail]/Drafts")
        );
    }

    #[test]
    fn test_sent_folder_db_cache() {
        let db = Database::open_memory().unwrap();

        assert!(db.get_sent_folder("acct1").unwrap().is_none());

        db.set_detected_folder("acct1", "sent", "Sent").unwrap();
        assert_eq!(
            db.get_sent_folder("acct1").unwrap().as_deref(),
            Some("Sent")
        );

        // Gmail-style
        db.set_detected_folder("acct1", "sent", "[Gmail]/Sent Mail").unwrap();
        assert_eq!(
            db.get_sent_folder("acct1").unwrap().as_deref(),
            Some("[Gmail]/Sent Mail")
        );
    }
}
