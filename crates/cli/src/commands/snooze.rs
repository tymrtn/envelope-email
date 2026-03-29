// Copyright (c) 2026 Tyler Martin
// Licensed under FSL-1.1-ALv2 (see LICENSE)

use anyhow::{Context, Result, bail};
use chrono::{Datelike, Duration, Local, NaiveDateTime, NaiveTime, Weekday};
use envelope_email_store::Database;
use envelope_email_store::credential_store::CredentialBackend;
use envelope_email_transport::imap;
use tracing::{info, warn};

use super::common::{resolve_account, setup_credentials};

/// The default IMAP folder name for snoozed messages.
const IMAP_SNOOZED_FOLDER: &str = "Snoozed";

/// Valid snooze reasons.
const VALID_REASONS: &[&str] = &["follow-up", "waiting-reply", "defer", "reminder", "review"];

/// Parse a flexible datetime string into an ISO8601 datetime.
///
/// Accepts:
/// - ISO8601: "2026-03-30T09:00:00"
/// - Relative: "2h", "3d", "1w", "30m"
/// - Natural: "tomorrow", "monday", "tuesday", ..., "next week"
fn parse_until(input: &str) -> Result<String> {
    let trimmed = input.trim();
    let now = Local::now();

    // Try ISO8601 first
    if let Ok(dt) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S") {
        return Ok(dt.format("%Y-%m-%dT%H:%M:%S").to_string());
    }

    // Try relative: number + unit suffix
    if trimmed.len() >= 2 {
        let (num_part, unit) = trimmed.split_at(trimmed.len() - 1);
        if let Ok(n) = num_part.parse::<i64>() {
            let duration = match unit {
                "m" => Duration::minutes(n),
                "h" => Duration::hours(n),
                "d" => Duration::days(n),
                "w" => Duration::weeks(n),
                _ => bail!("unknown time unit: '{unit}' (use m/h/d/w)"),
            };
            let target = now + duration;
            return Ok(target.format("%Y-%m-%dT%H:%M:%S").to_string());
        }
    }

    // Try natural language
    let lower = trimmed.to_lowercase();
    let morning = NaiveTime::from_hms_opt(9, 0, 0).unwrap();

    match lower.as_str() {
        "tomorrow" => {
            let target = (now + Duration::days(1)).date_naive().and_time(morning);
            Ok(target.format("%Y-%m-%dT%H:%M:%S").to_string())
        }
        "next week" => {
            // Next Monday at 09:00
            let days_until_monday = (8 - now.weekday().num_days_from_monday()) % 7;
            let days = if days_until_monday == 0 {
                7
            } else {
                days_until_monday as i64
            };
            let target = (now + Duration::days(days)).date_naive().and_time(morning);
            Ok(target.format("%Y-%m-%dT%H:%M:%S").to_string())
        }
        day_name => {
            // Try parsing as a day of the week
            let target_weekday = match day_name {
                "monday" | "mon" => Some(Weekday::Mon),
                "tuesday" | "tue" => Some(Weekday::Tue),
                "wednesday" | "wed" => Some(Weekday::Wed),
                "thursday" | "thu" => Some(Weekday::Thu),
                "friday" | "fri" => Some(Weekday::Fri),
                "saturday" | "sat" => Some(Weekday::Sat),
                "sunday" | "sun" => Some(Weekday::Sun),
                _ => None,
            };

            if let Some(wd) = target_weekday {
                let current = now.weekday().num_days_from_monday();
                let target = wd.num_days_from_monday();
                let days = if target > current {
                    (target - current) as i64
                } else {
                    (7 - current + target) as i64
                };
                let target_dt = (now + Duration::days(days)).date_naive().and_time(morning);
                Ok(target_dt.format("%Y-%m-%dT%H:%M:%S").to_string())
            } else {
                bail!(
                    "cannot parse '{trimmed}' as a datetime. \
                     Use ISO8601 (2026-03-30T09:00:00), relative (2h, 3d, 1w), \
                     or natural (tomorrow, monday, next week)"
                )
            }
        }
    }
}

/// Validate a snooze reason against the allowed set.
fn validate_reason(reason: &str) -> Result<()> {
    if VALID_REASONS.contains(&reason) {
        Ok(())
    } else {
        bail!(
            "invalid reason '{}'. Must be one of: {}",
            reason,
            VALID_REASONS.join(", ")
        )
    }
}

/// Snooze a message: move it to the Snoozed folder and record in local DB.
#[tokio::main]
pub async fn run_snooze(
    uid: u32,
    until: &str,
    folder: &str,
    account: Option<&str>,
    reason: Option<&str>,
    note: Option<&str>,
    recipient: Option<&str>,
    json: bool,
    backend: CredentialBackend,
) -> Result<()> {
    let return_at = parse_until(until).context("failed to parse --until value")?;

    // Validate reason if provided
    if let Some(r) = reason {
        validate_reason(r)?;
    }

    let (db, creds) = setup_credentials(account, backend)?;
    let account_email = &creds.account.username;

    // Check if already snoozed
    if let Some(existing) = db
        .find_snoozed_by_uid(account_email, uid)
        .context("database error")?
    {
        bail!(
            "UID {uid} is already snoozed (returns at {}). Unsnooze first.",
            existing.return_at
        );
    }

    // Connect to IMAP
    let mut client = imap::connect(&creds)
        .await
        .context("IMAP connection failed")?;

    // Ensure the Snoozed folder exists
    if let Err(e) = imap::create_folder(&mut client, IMAP_SNOOZED_FOLDER).await {
        warn!("could not create Snoozed folder (may already exist): {e}");
    }

    // Fetch message summary to store subject and message-id
    let msg = imap::fetch_message(&mut client, folder, uid)
        .await
        .context("failed to fetch message")?;

    let (subject, message_id) = match &msg {
        Some(m) => (Some(m.subject.as_str()), m.message_id.as_deref()),
        None => (None, None),
    };

    // Move message to Snoozed folder
    imap::move_message(&mut client, uid, folder, IMAP_SNOOZED_FOLDER)
        .await
        .context("failed to move message to Snoozed folder")?;

    info!("moved UID {uid} from {folder} to {IMAP_SNOOZED_FOLDER}");

    // Record in local DB
    let snoozed = db
        .create_snoozed(
            account_email,
            uid,
            folder,
            IMAP_SNOOZED_FOLDER,
            &return_at,
            message_id,
            subject,
            reason,
            note,
            recipient,
        )
        .context("failed to record snooze in database")?;

    if json {
        println!("{}", serde_json::to_string_pretty(&snoozed)?);
    } else {
        println!("Snoozed UID {uid}");
        println!("  From:     {folder}");
        println!("  Until:    {return_at}");
        if let Some(s) = subject {
            println!("  Subject:  {s}");
        }
        if let Some(r) = reason {
            println!("  Reason:   {r}");
        }
        if let Some(n) = note {
            println!("  Note:     {n}");
        }
        if let Some(r) = recipient {
            println!("  Waiting:  {r}");
        }
        println!("  ID:       {}", snoozed.id);
    }

    Ok(())
}

/// List all snoozed messages.
pub fn run_list(account: Option<&str>, json: bool, _backend: CredentialBackend) -> Result<()> {
    let db = Database::open_default().context("failed to open database")?;

    // Resolve account email if provided
    let account_filter = match account {
        Some(a) => {
            let acct = resolve_account(&db, Some(a))?;
            Some(acct.username)
        }
        None => None,
    };

    let snoozed = db
        .list_snoozed(account_filter.as_deref())
        .context("failed to list snoozed messages")?;

    if json {
        println!("{}", serde_json::to_string_pretty(&snoozed)?);
    } else {
        if snoozed.is_empty() {
            println!("No snoozed messages");
            return Ok(());
        }

        println!(
            "{:<6}  {:<25}  {:<22}  {:<14}  {:<20}  {}",
            "UID", "ACCOUNT", "RETURN AT", "REASON", "SUBJECT", "NOTE"
        );
        println!("{}", "-".repeat(120));
        for s in &snoozed {
            let subject = s.subject.as_deref().unwrap_or("-");
            let subject_display = if subject.len() > 18 {
                format!("{}...", &subject[..15])
            } else {
                subject.to_string()
            };
            let account_display = if s.account.len() > 23 {
                format!("{}...", &s.account[..20])
            } else {
                s.account.clone()
            };
            let reason_display = s.reason.as_deref().unwrap_or("-");
            let note_display = match s.note.as_deref() {
                Some(n) if n.len() > 30 => format!("{}...", &n[..27]),
                Some(n) => n.to_string(),
                None => "-".to_string(),
            };
            println!(
                "{:<6}  {:<25}  {:<22}  {:<14}  {:<20}  {}",
                s.uid, account_display, s.return_at, reason_display, subject_display, note_display,
            );
        }
        println!("\n{} snoozed message(s)", snoozed.len());
    }

    Ok(())
}

/// Unsnooze a message: move it back to its original folder immediately.
#[tokio::main]
pub async fn run_unsnooze(
    uid: u32,
    account: Option<&str>,
    json: bool,
    backend: CredentialBackend,
) -> Result<()> {
    let (db, creds) = setup_credentials(account, backend)?;
    let account_email = &creds.account.username;

    // Find the snoozed record
    let snoozed = db
        .find_snoozed_by_uid(account_email, uid)
        .context("database error")?
        .ok_or_else(|| {
            anyhow::anyhow!("no snoozed message found for UID {uid} on account {account_email}")
        })?;

    // Connect to IMAP and move back
    let mut client = imap::connect(&creds)
        .await
        .context("IMAP connection failed")?;

    // Find the message in the snoozed folder — UID may have changed after move
    // Search by message-id if available, otherwise try the stored UID
    let snoozed_uid = if let Some(ref mid) = snoozed.message_id {
        let mid_clean = mid.trim_matches(|c| c == '<' || c == '>');
        match imap::find_uid_by_message_id(&mut client, &snoozed.snoozed_folder, mid_clean).await {
            Ok(Some(uid)) => uid,
            Ok(None) => {
                warn!("message-id search returned no results, trying stored UID");
                snoozed.uid
            }
            Err(e) => {
                warn!("message-id search failed: {e}, trying stored UID");
                snoozed.uid
            }
        }
    } else {
        snoozed.uid
    };

    imap::move_message(
        &mut client,
        snoozed_uid,
        &snoozed.snoozed_folder,
        &snoozed.original_folder,
    )
    .await
    .context("failed to move message back to original folder")?;

    // Remove from local DB
    db.delete_snoozed(&snoozed.id)
        .context("failed to remove snooze record")?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "action": "unsnooze",
                "uid": uid,
                "from": snoozed.snoozed_folder,
                "to": snoozed.original_folder,
                "subject": snoozed.subject,
            })
        );
    } else {
        println!("Unsnoozed UID {uid}");
        println!("  Moved back to: {}", snoozed.original_folder);
        if let Some(ref s) = snoozed.subject {
            println!("  Subject: {s}");
        }
    }

    Ok(())
}

/// Check all snoozed messages and unsnooze any that are past their return time.
/// JSON output includes reason/note/escalation_tier for agent consumption.
#[tokio::main]
pub async fn run_check(
    account: Option<&str>,
    json: bool,
    backend: CredentialBackend,
) -> Result<()> {
    let db = Database::open_default().context("failed to open database")?;
    let now = Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();

    // Resolve account filter
    let account_filter = match account {
        Some(a) => {
            let acct = resolve_account(&db, Some(a))?;
            Some(acct.username.clone())
        }
        None => None,
    };

    let due = db
        .list_snoozed_due(&now, account_filter.as_deref())
        .context("failed to query due snoozed messages")?;

    if due.is_empty() {
        if json {
            println!("{}", serde_json::json!({ "unsnoozed": [] }));
        } else {
            println!("No snoozed messages due for return");
        }
        return Ok(());
    }

    // We need credentials per account to connect to IMAP.
    // Group by account and process each.
    let passphrase = envelope_email_store::credential_store::get_or_create_passphrase(backend)
        .context("credential store error")?;

    let mut results: Vec<serde_json::Value> = Vec::new();
    let mut success_count = 0u32;
    let mut fail_count = 0u32;

    // Group messages by account
    let mut by_account: std::collections::HashMap<String, Vec<_>> =
        std::collections::HashMap::new();
    for msg in &due {
        by_account.entry(msg.account.clone()).or_default().push(msg);
    }

    for (account_email, messages) in &by_account {
        // Resolve account and get credentials
        let acct = match db
            .find_account_by_email(account_email)
            .context("database error")?
        {
            Some(a) => a,
            None => {
                warn!("account not found for snoozed messages: {account_email}");
                fail_count += messages.len() as u32;
                continue;
            }
        };

        let creds = match db.get_account_with_credentials(&acct.id, &passphrase) {
            Ok(c) => c,
            Err(e) => {
                warn!("failed to decrypt credentials for {account_email}: {e}");
                fail_count += messages.len() as u32;
                continue;
            }
        };

        let mut client = match imap::connect(&creds).await {
            Ok(c) => c,
            Err(e) => {
                warn!("IMAP connection failed for {account_email}: {e}");
                fail_count += messages.len() as u32;
                continue;
            }
        };

        for msg in messages {
            // Find the actual UID in the snoozed folder
            let snoozed_uid = if let Some(ref mid) = msg.message_id {
                let mid_clean = mid.trim_matches(|c| c == '<' || c == '>');
                match imap::find_uid_by_message_id(&mut client, &msg.snoozed_folder, mid_clean)
                    .await
                {
                    Ok(Some(uid)) => uid,
                    _ => msg.uid,
                }
            } else {
                msg.uid
            };

            match imap::move_message(
                &mut client,
                snoozed_uid,
                &msg.snoozed_folder,
                &msg.original_folder,
            )
            .await
            {
                Ok(()) => {
                    if let Err(e) = db.delete_snoozed(&msg.id) {
                        warn!("failed to delete snooze record {}: {e}", msg.id);
                    }
                    success_count += 1;
                    info!("unsnoozed UID {} -> {}", msg.uid, msg.original_folder);

                    if !json {
                        let subj = msg.subject.as_deref().unwrap_or("-");
                        println!("  ✓ UID {} → {} ({})", msg.uid, msg.original_folder, subj);
                    }

                    results.push(serde_json::json!({
                        "uid": msg.uid,
                        "account": msg.account,
                        "to": msg.original_folder,
                        "subject": msg.subject,
                        "reason": msg.reason,
                        "note": msg.note,
                        "recipient": msg.recipient,
                        "escalation_tier": msg.escalation_tier,
                        "reply_received": msg.reply_received,
                        "status": "ok",
                    }));
                }
                Err(e) => {
                    warn!("failed to unsnooze UID {}: {e}", msg.uid);
                    fail_count += 1;

                    if !json {
                        println!("  ✗ UID {} — {e}", msg.uid);
                    }

                    results.push(serde_json::json!({
                        "uid": msg.uid,
                        "account": msg.account,
                        "error": format!("{e}"),
                        "reason": msg.reason,
                        "note": msg.note,
                        "escalation_tier": msg.escalation_tier,
                        "status": "error",
                    }));
                }
            }
        }
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "unsnoozed": results,
                "success": success_count,
                "failed": fail_count,
            }))?
        );
    } else {
        println!("\n{success_count} unsnoozed, {fail_count} failed");
    }

    Ok(())
}

/// Check for replies to snoozed messages that are waiting for a response.
///
/// For each snoozed message with reason "waiting-reply" or "follow-up":
/// 1. Extract the recipient email from the snoozed record
/// 2. Search ALL configured accounts' inboxes for recent messages FROM that recipient
/// 3. If a reply is found: mark reply_received = 1, output the match
/// 4. If no reply and past the --until time: increment escalation_tier
/// 5. JSON output for agent consumption
#[tokio::main]
pub async fn run_check_replies(
    account: Option<&str>,
    json: bool,
    backend: CredentialBackend,
) -> Result<()> {
    let db = Database::open_default().context("failed to open database")?;
    let now = Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();

    // Resolve account filter
    let account_filter = match account {
        Some(a) => {
            let acct = resolve_account(&db, Some(a))?;
            Some(acct.username.clone())
        }
        None => None,
    };

    // Get all snoozed messages awaiting reply
    let awaiting = db
        .list_snoozed_awaiting_reply(account_filter.as_deref())
        .context("failed to query snoozed messages awaiting reply")?;

    if awaiting.is_empty() {
        if json {
            println!("{}", serde_json::json!({ "results": [], "checked": 0 }));
        } else {
            println!("No snoozed messages awaiting replies");
        }
        return Ok(());
    }

    // Collect unique recipient emails to search for
    let recipients_to_check: Vec<(String, Vec<&envelope_email_store::SnoozedMessage>)> = {
        let mut by_recipient: std::collections::HashMap<
            String,
            Vec<&envelope_email_store::SnoozedMessage>,
        > = std::collections::HashMap::new();
        for msg in &awaiting {
            if let Some(ref recip) = msg.recipient {
                by_recipient.entry(recip.clone()).or_default().push(msg);
            }
        }
        by_recipient.into_iter().collect()
    };

    // Get all configured accounts for cross-account search
    let all_accounts = db.list_accounts().context("failed to list accounts")?;
    let passphrase = envelope_email_store::credential_store::get_or_create_passphrase(backend)
        .context("credential store error")?;

    // For each account, connect and search for messages FROM each recipient
    let mut reply_found_for: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut reply_details: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();

    for acct in &all_accounts {
        let creds = match db.get_account_with_credentials(&acct.id, &passphrase) {
            Ok(c) => c,
            Err(e) => {
                warn!("failed to decrypt credentials for {}: {e}", acct.username);
                continue;
            }
        };

        let mut client = match imap::connect(&creds).await {
            Ok(c) => c,
            Err(e) => {
                warn!("IMAP connection failed for {}: {e}", acct.username);
                continue;
            }
        };

        for (recipient, _msgs) in &recipients_to_check {
            if reply_found_for.contains(recipient) {
                continue; // Already found a reply for this recipient
            }

            // IMAP search: FROM <recipient> in INBOX, last 30 days
            let search_query = format!("FROM \"{}\" SINCE 1-Jan-2026", recipient);
            match imap::search(&mut client, "INBOX", &search_query, 5).await {
                Ok(results) if !results.is_empty() => {
                    let newest = &results[0];
                    reply_found_for.insert(recipient.clone());
                    reply_details.insert(
                        recipient.clone(),
                        serde_json::json!({
                            "found_in_account": acct.username,
                            "from": newest.from_addr,
                            "subject": newest.subject,
                            "date": newest.date,
                            "uid": newest.uid,
                        }),
                    );
                    info!(
                        "found reply from {recipient} in {} (UID {})",
                        acct.username, newest.uid
                    );
                }
                Ok(_) => {} // No results
                Err(e) => {
                    warn!(
                        "search failed for FROM {} in {}: {e}",
                        recipient, acct.username
                    );
                }
            }
        }
    }

    // Now process results: update DB and build output
    let mut results: Vec<serde_json::Value> = Vec::new();

    for msg in &awaiting {
        let recipient = msg.recipient.as_deref().unwrap_or("(unknown)");

        if let Some(reply_info) = msg.recipient.as_ref().and_then(|r| reply_details.get(r)) {
            // Reply found — mark in DB
            if let Err(e) = db.mark_reply_received(&msg.id) {
                warn!("failed to mark reply_received for {}: {e}", msg.id);
            }

            if !json {
                println!(
                    "  ✓ UID {} — reply from {} found! {}",
                    msg.uid,
                    recipient,
                    reply_info
                        .get("subject")
                        .and_then(|s| s.as_str())
                        .unwrap_or("-")
                );
            }

            results.push(serde_json::json!({
                "uid": msg.uid,
                "account": msg.account,
                "subject": msg.subject,
                "reason": msg.reason,
                "note": msg.note,
                "recipient": recipient,
                "status": "reply_found",
                "reply": reply_info,
                "escalation_tier": msg.escalation_tier,
            }));
        } else if msg.recipient.is_none() {
            // No recipient recorded — can't check
            if !json {
                println!(
                    "  ? UID {} — no recipient recorded, cannot check replies",
                    msg.uid
                );
            }
            results.push(serde_json::json!({
                "uid": msg.uid,
                "account": msg.account,
                "subject": msg.subject,
                "reason": msg.reason,
                "note": msg.note,
                "recipient": null,
                "status": "no_recipient",
                "escalation_tier": msg.escalation_tier,
            }));
        } else {
            // No reply found
            let is_overdue = msg.return_at <= now;
            let new_tier = if is_overdue {
                // Past due — escalate
                match db.increment_escalation(&msg.id) {
                    Ok(t) => t,
                    Err(e) => {
                        warn!("failed to increment escalation for {}: {e}", msg.id);
                        msg.escalation_tier
                    }
                }
            } else {
                msg.escalation_tier
            };

            if !json {
                if is_overdue {
                    println!(
                        "  ✗ UID {} — no reply from {} (overdue, escalation tier {})",
                        msg.uid, recipient, new_tier
                    );
                } else {
                    println!(
                        "  … UID {} — no reply from {} yet (due {})",
                        msg.uid, recipient, msg.return_at
                    );
                }
            }

            results.push(serde_json::json!({
                "uid": msg.uid,
                "account": msg.account,
                "subject": msg.subject,
                "reason": msg.reason,
                "note": msg.note,
                "recipient": recipient,
                "status": if is_overdue { "no_reply_overdue" } else { "no_reply_waiting" },
                "escalation_tier": new_tier,
                "overdue": is_overdue,
                "return_at": msg.return_at,
            }));
        }
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "results": results,
                "checked": awaiting.len(),
            }))?
        );
    } else {
        println!(
            "\nChecked {} snoozed message(s) awaiting replies",
            awaiting.len()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_iso8601() {
        let result = parse_until("2026-03-30T09:00:00").unwrap();
        assert_eq!(result, "2026-03-30T09:00:00");
    }

    #[test]
    fn parse_relative_hours() {
        let result = parse_until("2h").unwrap();
        // Should be roughly 2 hours from now — just verify it parses
        assert!(result.contains("T"));
    }

    #[test]
    fn parse_relative_days() {
        let result = parse_until("3d").unwrap();
        assert!(result.contains("T"));
    }

    #[test]
    fn parse_relative_weeks() {
        let result = parse_until("1w").unwrap();
        assert!(result.contains("T"));
    }

    #[test]
    fn parse_tomorrow() {
        let result = parse_until("tomorrow").unwrap();
        assert!(result.contains("T09:00:00"));
    }

    #[test]
    fn parse_day_name() {
        let result = parse_until("monday").unwrap();
        assert!(result.contains("T09:00:00"));
    }

    #[test]
    fn parse_next_week() {
        let result = parse_until("next week").unwrap();
        assert!(result.contains("T09:00:00"));
    }

    #[test]
    fn parse_invalid() {
        assert!(parse_until("banana").is_err());
    }

    #[test]
    fn validate_reason_valid() {
        assert!(validate_reason("follow-up").is_ok());
        assert!(validate_reason("waiting-reply").is_ok());
        assert!(validate_reason("defer").is_ok());
        assert!(validate_reason("reminder").is_ok());
        assert!(validate_reason("review").is_ok());
    }

    #[test]
    fn validate_reason_invalid() {
        assert!(validate_reason("banana").is_err());
        assert!(validate_reason("").is_err());
    }
}
