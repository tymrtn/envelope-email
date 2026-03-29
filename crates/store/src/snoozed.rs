// Copyright (c) 2026 Tyler Martin
// Licensed under FSL-1.1-ALv2 (see LICENSE)

use crate::db::Database;
use crate::errors::Result;
use crate::models::SnoozedMessage;
use rusqlite::params;
use uuid::Uuid;

/// The full SELECT column list for snoozed messages (v2 schema).
const SNOOZED_COLS: &str = "id, account, uid, original_folder, snoozed_folder, \
     return_at, message_id, subject, created_at, \
     reason, note, recipient, escalation_tier, reply_received";

impl Database {
    /// Record a snoozed message in the local database.
    pub fn create_snoozed(
        &self,
        account: &str,
        uid: u32,
        original_folder: &str,
        snoozed_folder: &str,
        return_at: &str,
        message_id: Option<&str>,
        subject: Option<&str>,
        reason: Option<&str>,
        note: Option<&str>,
        recipient: Option<&str>,
    ) -> Result<SnoozedMessage> {
        let id = Uuid::new_v4().to_string();

        self.conn().execute(
            "INSERT INTO snoozed (id, account, uid, original_folder, snoozed_folder,
             return_at, message_id, subject, reason, note, recipient)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id,
                account,
                uid as i64,
                original_folder,
                snoozed_folder,
                return_at,
                message_id,
                subject,
                reason,
                note,
                recipient
            ],
        )?;

        self.get_snoozed(&id)?.ok_or_else(|| {
            crate::errors::StoreError::Config(format!(
                "snoozed message not found after insert: {id}"
            ))
        })
    }

    /// Get a single snoozed message by ID.
    pub fn get_snoozed(&self, id: &str) -> Result<Option<SnoozedMessage>> {
        let sql = format!("SELECT {SNOOZED_COLS} FROM snoozed WHERE id = ?1");
        let mut stmt = self.conn().prepare(&sql)?;

        let msg = stmt.query_row(params![id], Self::map_snoozed).optional()?;

        Ok(msg)
    }

    /// Find a snoozed record by original UID and account.
    pub fn find_snoozed_by_uid(&self, account: &str, uid: u32) -> Result<Option<SnoozedMessage>> {
        let sql = format!("SELECT {SNOOZED_COLS} FROM snoozed WHERE account = ?1 AND uid = ?2");
        let mut stmt = self.conn().prepare(&sql)?;

        let msg = stmt
            .query_row(params![account, uid as i64], Self::map_snoozed)
            .optional()?;

        Ok(msg)
    }

    /// List all snoozed messages, optionally filtered by account.
    pub fn list_snoozed(&self, account: Option<&str>) -> Result<Vec<SnoozedMessage>> {
        let (sql, account_param) = match account {
            Some(a) => (
                format!(
                    "SELECT {SNOOZED_COLS} FROM snoozed WHERE account = ?1 ORDER BY return_at ASC"
                ),
                a.to_string(),
            ),
            None => (
                format!("SELECT {SNOOZED_COLS} FROM snoozed ORDER BY return_at ASC"),
                String::new(),
            ),
        };

        let mut stmt = self.conn().prepare(&sql)?;
        let rows = if account.is_some() {
            stmt.query_map(params![account_param], Self::map_snoozed)?
        } else {
            stmt.query_map([], Self::map_snoozed)?
        };

        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// List snoozed messages whose return_at is past a given ISO8601 datetime.
    pub fn list_snoozed_due(
        &self,
        now: &str,
        account: Option<&str>,
    ) -> Result<Vec<SnoozedMessage>> {
        let messages = match account {
            Some(a) => {
                let sql = format!(
                    "SELECT {SNOOZED_COLS} FROM snoozed \
                     WHERE return_at <= ?1 AND account = ?2 ORDER BY return_at ASC"
                );
                let mut stmt = self.conn().prepare(&sql)?;
                let rows = stmt.query_map(params![now, a], Self::map_snoozed)?;
                rows.filter_map(|r| r.ok()).collect()
            }
            None => {
                let sql = format!(
                    "SELECT {SNOOZED_COLS} FROM snoozed \
                     WHERE return_at <= ?1 ORDER BY return_at ASC"
                );
                let mut stmt = self.conn().prepare(&sql)?;
                let rows = stmt.query_map(params![now], Self::map_snoozed)?;
                rows.filter_map(|r| r.ok()).collect()
            }
        };

        Ok(messages)
    }

    /// List snoozed messages with reason "waiting-reply" or "follow-up" that haven't
    /// received a reply yet, optionally filtered by account.
    pub fn list_snoozed_awaiting_reply(
        &self,
        account: Option<&str>,
    ) -> Result<Vec<SnoozedMessage>> {
        let messages = match account {
            Some(a) => {
                let sql = format!(
                    "SELECT {SNOOZED_COLS} FROM snoozed \
                     WHERE (reason = 'waiting-reply' OR reason = 'follow-up') \
                     AND reply_received = 0 AND account = ?1 \
                     ORDER BY return_at ASC"
                );
                let mut stmt = self.conn().prepare(&sql)?;
                let rows = stmt.query_map(params![a], Self::map_snoozed)?;
                rows.filter_map(|r| r.ok()).collect()
            }
            None => {
                let sql = format!(
                    "SELECT {SNOOZED_COLS} FROM snoozed \
                     WHERE (reason = 'waiting-reply' OR reason = 'follow-up') \
                     AND reply_received = 0 \
                     ORDER BY return_at ASC"
                );
                let mut stmt = self.conn().prepare(&sql)?;
                let rows = stmt.query_map([], Self::map_snoozed)?;
                rows.filter_map(|r| r.ok()).collect()
            }
        };

        Ok(messages)
    }

    /// Mark a snoozed message as having received a reply.
    pub fn mark_reply_received(&self, id: &str) -> Result<bool> {
        let rows = self.conn().execute(
            "UPDATE snoozed SET reply_received = 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(rows > 0)
    }

    /// Increment the escalation tier for a snoozed message.
    pub fn increment_escalation(&self, id: &str) -> Result<i32> {
        self.conn().execute(
            "UPDATE snoozed SET escalation_tier = escalation_tier + 1 WHERE id = ?1",
            params![id],
        )?;
        // Return the new tier
        let tier: i32 = self.conn().query_row(
            "SELECT escalation_tier FROM snoozed WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        Ok(tier)
    }

    /// Delete a snoozed record by ID (used after unsnoozing).
    pub fn delete_snoozed(&self, id: &str) -> Result<bool> {
        let rows = self
            .conn()
            .execute("DELETE FROM snoozed WHERE id = ?1", params![id])?;
        Ok(rows > 0)
    }

    fn map_snoozed(row: &rusqlite::Row<'_>) -> rusqlite::Result<SnoozedMessage> {
        let uid_i64: i64 = row.get(2)?;
        let escalation_tier: i32 = row.get(12)?;
        let reply_received_int: i32 = row.get(13)?;
        Ok(SnoozedMessage {
            id: row.get(0)?,
            account: row.get(1)?,
            uid: uid_i64 as u32,
            original_folder: row.get(3)?,
            snoozed_folder: row.get(4)?,
            return_at: row.get(5)?,
            message_id: row.get(6)?,
            subject: row.get(7)?,
            created_at: row.get(8)?,
            reason: row.get(9)?,
            note: row.get(10)?,
            recipient: row.get(11)?,
            escalation_tier,
            reply_received: reply_received_int != 0,
        })
    }
}

trait OptionalExt<T> {
    fn optional(self) -> std::result::Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for std::result::Result<T, rusqlite::Error> {
    fn optional(self) -> std::result::Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::db::Database;

    #[test]
    fn create_and_list_snoozed() {
        let db = Database::open_memory().unwrap();
        let snoozed = db
            .create_snoozed(
                "test@test.com",
                12345,
                "INBOX",
                "Snoozed",
                "2026-04-01T09:00:00",
                Some("<msg@test.com>"),
                Some("Test Subject"),
                None,
                None,
                None,
            )
            .unwrap();

        assert_eq!(snoozed.uid, 12345);
        assert_eq!(snoozed.original_folder, "INBOX");
        assert_eq!(snoozed.escalation_tier, 0);
        assert!(!snoozed.reply_received);

        let all = db.list_snoozed(Some("test@test.com")).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].uid, 12345);
    }

    #[test]
    fn create_with_reason_and_note() {
        let db = Database::open_memory().unwrap();
        let snoozed = db
            .create_snoozed(
                "test@test.com",
                100,
                "INBOX",
                "Snoozed",
                "2026-04-01T09:00:00",
                None,
                Some("Test Subject"),
                Some("follow-up"),
                Some("Check if Mark replied"),
                Some("mark@example.com"),
            )
            .unwrap();

        assert_eq!(snoozed.reason.as_deref(), Some("follow-up"));
        assert_eq!(snoozed.note.as_deref(), Some("Check if Mark replied"));
        assert_eq!(snoozed.recipient.as_deref(), Some("mark@example.com"));
        assert_eq!(snoozed.escalation_tier, 0);
        assert!(!snoozed.reply_received);
    }

    #[test]
    fn list_snoozed_due() {
        let db = Database::open_memory().unwrap();
        db.create_snoozed(
            "test@test.com",
            100,
            "INBOX",
            "Snoozed",
            "2026-03-01T09:00:00",
            None,
            Some("Past"),
            None,
            None,
            None,
        )
        .unwrap();
        db.create_snoozed(
            "test@test.com",
            200,
            "INBOX",
            "Snoozed",
            "2099-12-31T23:59:59",
            None,
            Some("Future"),
            None,
            None,
            None,
        )
        .unwrap();

        let due = db.list_snoozed_due("2026-06-01T00:00:00", None).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].uid, 100);
    }

    #[test]
    fn delete_snoozed() {
        let db = Database::open_memory().unwrap();
        let snoozed = db
            .create_snoozed(
                "test@test.com",
                999,
                "INBOX",
                "Snoozed",
                "2026-04-01T09:00:00",
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        assert!(db.delete_snoozed(&snoozed.id).unwrap());
        assert!(db.list_snoozed(None).unwrap().is_empty());
    }

    #[test]
    fn find_by_uid() {
        let db = Database::open_memory().unwrap();
        db.create_snoozed(
            "test@test.com",
            555,
            "INBOX",
            "Snoozed",
            "2026-04-01T09:00:00",
            None,
            Some("Find me"),
            None,
            None,
            None,
        )
        .unwrap();

        let found = db.find_snoozed_by_uid("test@test.com", 555).unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().subject.as_deref(), Some("Find me"));

        let not_found = db.find_snoozed_by_uid("other@test.com", 555).unwrap();
        assert!(not_found.is_none());
    }

    #[test]
    fn list_awaiting_reply() {
        let db = Database::open_memory().unwrap();
        db.create_snoozed(
            "test@test.com",
            100,
            "INBOX",
            "Snoozed",
            "2026-04-01T09:00:00",
            None,
            Some("Follow-up"),
            Some("follow-up"),
            Some("Check reply"),
            Some("mark@example.com"),
        )
        .unwrap();
        db.create_snoozed(
            "test@test.com",
            200,
            "INBOX",
            "Snoozed",
            "2026-04-01T09:00:00",
            None,
            Some("Deferred"),
            Some("defer"),
            None,
            None,
        )
        .unwrap();
        db.create_snoozed(
            "test@test.com",
            300,
            "INBOX",
            "Snoozed",
            "2026-04-01T09:00:00",
            None,
            Some("Waiting"),
            Some("waiting-reply"),
            None,
            Some("jane@example.com"),
        )
        .unwrap();

        let awaiting = db.list_snoozed_awaiting_reply(None).unwrap();
        assert_eq!(awaiting.len(), 2);
        assert_eq!(awaiting[0].uid, 100);
        assert_eq!(awaiting[1].uid, 300);
    }

    #[test]
    fn mark_reply_and_escalation() {
        let db = Database::open_memory().unwrap();
        let snoozed = db
            .create_snoozed(
                "test@test.com",
                100,
                "INBOX",
                "Snoozed",
                "2026-04-01T09:00:00",
                None,
                Some("Test"),
                Some("follow-up"),
                None,
                Some("mark@example.com"),
            )
            .unwrap();

        // Mark reply received
        assert!(db.mark_reply_received(&snoozed.id).unwrap());
        let updated = db.get_snoozed(&snoozed.id).unwrap().unwrap();
        assert!(updated.reply_received);

        // Increment escalation
        let tier = db.increment_escalation(&snoozed.id).unwrap();
        assert_eq!(tier, 1);
        let tier2 = db.increment_escalation(&snoozed.id).unwrap();
        assert_eq!(tier2, 2);
    }
}
