// Copyright (c) 2026 Tyler Martin
// Licensed under FSL-1.1-ALv2 (see LICENSE)

use crate::db::Database;
use crate::errors::Result;
use crate::models::Event;

impl Database {
    /// Insert an event into the events table.
    pub fn insert_event(&self, event: &Event) -> Result<()> {
        self.conn().execute(
            "INSERT INTO events (id, account_id, event_type, folder, uid, message_id, from_addr, subject, snippet, payload, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                event.id,
                event.account_id,
                event.event_type,
                event.folder,
                event.uid,
                event.message_id,
                event.from_addr,
                event.subject,
                event.snippet,
                event.payload,
                event.created_at,
            ],
        )?;
        Ok(())
    }

    /// List recent events, optionally filtered by account.
    pub fn list_events(
        &self,
        account_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Event>> {
        let (sql, params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = match account_id {
            Some(id) => (
                "SELECT id, account_id, event_type, folder, uid, message_id, from_addr, subject, snippet, payload, created_at
                 FROM events WHERE account_id = ?1 ORDER BY created_at DESC LIMIT ?2",
                vec![Box::new(id.to_string()), Box::new(limit as i64)],
            ),
            None => (
                "SELECT id, account_id, event_type, folder, uid, message_id, from_addr, subject, snippet, payload, created_at
                 FROM events ORDER BY created_at DESC LIMIT ?1",
                vec![Box::new(limit as i64)],
            ),
        };

        let mut stmt = self.conn().prepare(sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), |row: &rusqlite::Row| {
            Ok(Event {
                id: row.get(0)?,
                account_id: row.get(1)?,
                event_type: row.get(2)?,
                folder: row.get(3)?,
                uid: row.get(4)?,
                message_id: row.get(5)?,
                from_addr: row.get(6)?,
                subject: row.get(7)?,
                snippet: row.get(8)?,
                payload: row.get(9)?,
                created_at: row.get(10)?,
            })
        })?;
        Ok(rows.filter_map(|r: std::result::Result<Event, rusqlite::Error>| r.ok()).collect())
    }

    /// Check if there are recent events (within last N seconds).
    /// Used by `envelope code` to decide whether to tail events or poll IMAP.
    pub fn has_recent_events(&self, seconds: i64) -> Result<bool> {
        let count: i64 = self.conn().query_row(
            "SELECT COUNT(*) FROM events WHERE created_at >= datetime('now', ?1)",
            rusqlite::params![format!("-{seconds} seconds")],
            |row: &rusqlite::Row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// List events newer than a given timestamp for a specific account.
    pub fn list_events_since(
        &self,
        account_id: &str,
        since: &str,
    ) -> Result<Vec<Event>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, account_id, event_type, folder, uid, message_id, from_addr, subject, snippet, payload, created_at
             FROM events WHERE account_id = ?1 AND created_at > ?2 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(rusqlite::params![account_id, since], |row: &rusqlite::Row| {
            Ok(Event {
                id: row.get(0)?,
                account_id: row.get(1)?,
                event_type: row.get(2)?,
                folder: row.get(3)?,
                uid: row.get(4)?,
                message_id: row.get(5)?,
                from_addr: row.get(6)?,
                subject: row.get(7)?,
                snippet: row.get(8)?,
                payload: row.get(9)?,
                created_at: row.get(10)?,
            })
        })?;
        Ok(rows.filter_map(|r: std::result::Result<Event, rusqlite::Error>| r.ok()).collect())
    }

    /// Prune events older than N days.
    pub fn prune_events(&self, days: i64) -> Result<usize> {
        let deleted = self.conn().execute(
            "DELETE FROM events WHERE created_at < datetime('now', ?1)",
            rusqlite::params![format!("-{days} days")],
        )?;
        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Database {
        Database::open_memory().unwrap()
    }

    #[test]
    fn insert_and_list_events() {
        let db = test_db();
        let event = Event {
            id: "evt-1".to_string(),
            account_id: "acc-1".to_string(),
            event_type: "new_message".to_string(),
            folder: "INBOX".to_string(),
            uid: Some(42),
            message_id: Some("<msg@example.com>".to_string()),
            from_addr: Some("alice@example.com".to_string()),
            subject: Some("Hello".to_string()),
            snippet: Some("Hi there...".to_string()),
            payload: None,
            created_at: "2026-04-19T12:00:00".to_string(),
        };
        db.insert_event(&event).unwrap();

        let events = db.list_events(Some("acc-1"), 10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "new_message");
        assert_eq!(events[0].uid, Some(42));
    }

    #[test]
    fn list_events_filters_by_account() {
        let db = test_db();
        for (i, acc) in ["acc-1", "acc-2"].iter().enumerate() {
            db.insert_event(&Event {
                id: format!("evt-{i}"),
                account_id: acc.to_string(),
                event_type: "new_message".to_string(),
                folder: "INBOX".to_string(),
                uid: Some(i as i64),
                message_id: None,
                from_addr: None,
                subject: None,
                snippet: None,
                payload: None,
                created_at: "2026-04-19T12:00:00".to_string(),
            })
            .unwrap();
        }

        assert_eq!(db.list_events(Some("acc-1"), 10).unwrap().len(), 1);
        assert_eq!(db.list_events(None, 10).unwrap().len(), 2);
    }
}
