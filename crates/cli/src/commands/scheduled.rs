// Copyright (c) 2026 Tyler Martin
// Licensed under FSL-1.1-ALv2 (see LICENSE)

use anyhow::{Context, Result, bail};
use envelope_email_store::Database;
use envelope_email_store::credential_store::CredentialBackend;

use super::common::resolve_account;

/// List scheduled messages (drafts with send_after set, still in draft status).
pub fn run_list(account: Option<&str>, json: bool, _backend: CredentialBackend) -> Result<()> {
    let db = Database::open_default().context("failed to open database")?;

    // Resolve account if provided
    let account_id = match account {
        Some(a) => {
            let acct = resolve_account(&db, Some(a))?;
            Some(acct.id)
        }
        None => None,
    };

    // Query drafts with send_after set and status = 'draft'
    let drafts = if let Some(ref acct_id) = account_id {
        db.list_drafts(acct_id, Some("draft"), 100, 0)
            .context("failed to list drafts")?
            .into_iter()
            .filter(|d| d.send_after.is_some())
            .collect::<Vec<_>>()
    } else {
        // List all accounts and aggregate
        let accounts = db.list_accounts().context("failed to list accounts")?;
        let mut all = Vec::new();
        for acct in &accounts {
            let mut drafts = db
                .list_drafts(&acct.id, Some("draft"), 100, 0)
                .context("failed to list drafts")?
                .into_iter()
                .filter(|d| d.send_after.is_some())
                .collect::<Vec<_>>();
            all.append(&mut drafts);
        }
        all
    };

    if json {
        let items: Vec<serde_json::Value> = drafts
            .iter()
            .map(|d| {
                serde_json::json!({
                    "draft_id": d.id,
                    "account_id": d.account_id,
                    "to": d.to_addr,
                    "subject": d.subject,
                    "send_after": d.send_after,
                    "created_at": d.created_at,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else {
        if drafts.is_empty() {
            println!("No scheduled messages");
            return Ok(());
        }

        println!(
            "{:<36}  {:<28}  {:<22}  {}",
            "DRAFT ID", "TO", "SEND AT", "SUBJECT"
        );
        println!("{}", "-".repeat(110));
        for d in &drafts {
            let subject = d.subject.as_deref().unwrap_or("-");
            let subject_display = if subject.len() > 30 {
                format!("{}...", &subject[..27])
            } else {
                subject.to_string()
            };
            let to_display = if d.to_addr.len() > 26 {
                format!("{}...", &d.to_addr[..23])
            } else {
                d.to_addr.clone()
            };
            let send_at = d.send_after.as_deref().unwrap_or("-");
            println!(
                "{:<36}  {:<28}  {:<22}  {}",
                d.id, to_display, send_at, subject_display,
            );
        }
        println!("\n{} scheduled message(s)", drafts.len());
    }

    Ok(())
}

/// Cancel a scheduled message by discarding the draft.
pub fn run_cancel(id: &str, _account: Option<&str>, json: bool) -> Result<()> {
    let db = Database::open_default().context("failed to open database")?;

    // Verify the draft exists and has send_after
    let draft = db
        .get_draft(id)
        .context("failed to get draft")?
        .ok_or_else(|| anyhow::anyhow!("draft not found: {id}"))?;

    if draft.send_after.is_none() {
        bail!("draft {id} is not a scheduled message (no send_after set)");
    }

    let discarded = db.discard_draft(id).context("failed to discard draft")?;
    if !discarded {
        bail!(
            "could not cancel draft {id} (status: {})",
            draft.status.as_str()
        );
    }

    if json {
        println!(
            "{}",
            serde_json::json!({
                "action": "cancel",
                "draft_id": id,
                "to": draft.to_addr,
                "subject": draft.subject,
                "was_scheduled_for": draft.send_after,
            })
        );
    } else {
        println!("Cancelled scheduled message: {id}");
        println!("  To:       {}", draft.to_addr);
        if let Some(ref s) = draft.subject {
            println!("  Subject:  {s}");
        }
        if let Some(ref sa) = draft.send_after {
            println!("  Was scheduled for: {sa}");
        }
    }

    Ok(())
}
