// Copyright (c) 2026 Tyler Martin
// Licensed under FSL-1.1-ALv2 (see LICENSE)

//! `envelope code` — poll IMAP for a verification code and extract it.

use anyhow::{Context, Result};
use envelope_email_store::credential_store::CredentialBackend;
use envelope_email_transport::code_extractor::extract_code;
use envelope_email_transport::imap;

use super::common::setup_credentials;

/// `envelope code` — poll IMAP for new messages and extract a verification code.
///
/// Polls every 5 seconds for new messages matching the optional filters.
/// Exits 0 with the code on success, exits 1 on timeout.
#[tokio::main]
pub async fn run(
    account: Option<&str>,
    from_filter: Option<&str>,
    subject_filter: Option<&str>,
    wait_secs: u64,
    json: bool,
    backend: CredentialBackend,
) -> Result<()> {
    let (_db, creds) = setup_credentials(account, backend)?;

    let mut client = imap::connect(&creds)
        .await
        .context("IMAP connection failed")?;

    // SELECT INBOX and find the highest UID currently present
    client
        .session_mut()
        .select("INBOX")
        .await
        .map_err(|e| anyhow::anyhow!("SELECT INBOX: {e}"))?;

    let initial_max_uid = get_max_uid(&mut client).await?;

    if !json {
        eprintln!(
            "Watching for verification codes (timeout: {wait_secs}s, starting after UID {initial_max_uid})..."
        );
    }

    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(wait_secs);
    let poll_interval = std::time::Duration::from_secs(5);
    let mut last_seen_uid = initial_max_uid;

    loop {
        if start.elapsed() >= timeout {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"error": "timeout", "waited_seconds": wait_secs})
                );
            } else {
                eprintln!("Timeout: no verification code found after {wait_secs}s");
            }
            std::process::exit(1);
        }

        // Search for UIDs greater than last_seen
        let new_uids = search_new_uids(&mut client, last_seen_uid).await?;

        for uid in new_uids {
            if uid <= last_seen_uid {
                continue;
            }
            last_seen_uid = uid;

            // Fetch the full message
            let msg = match imap::fetch_message(&mut client, "INBOX", uid).await? {
                Some(m) => m,
                None => continue,
            };

            // Apply filters
            if let Some(from_pat) = from_filter {
                let from_lower = msg.from_addr.to_lowercase();
                let pat_lower = from_pat.to_lowercase();
                // Match domain or full address
                if !from_lower.contains(&pat_lower) {
                    continue;
                }
            }

            if let Some(subj_pat) = subject_filter {
                let subj_lower = msg.subject.to_lowercase();
                let pat_lower = subj_pat.to_lowercase();
                if !subj_lower.contains(&pat_lower) {
                    continue;
                }
            }

            // Try to extract a code
            let code = extract_code(
                msg.text_body.as_deref().unwrap_or(""),
                msg.html_body.as_deref(),
            );

            if let Some(code) = code {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "code": code,
                            "from": msg.from_addr,
                            "subject": msg.subject,
                        }))?
                    );
                } else {
                    println!("{code}");
                }
                return Ok(());
            }
        }

        tokio::time::sleep(poll_interval).await;
    }
}

/// Get the current maximum UID in INBOX.
async fn get_max_uid(client: &mut imap::ImapClient) -> Result<u32> {
    let uid_set = client
        .session_mut()
        .uid_search("ALL")
        .await
        .map_err(|e| anyhow::anyhow!("UID SEARCH ALL: {e}"))?;

    Ok(uid_set.into_iter().max().unwrap_or(0))
}

/// Search for UIDs greater than `since_uid` in the currently selected mailbox.
async fn search_new_uids(client: &mut imap::ImapClient, since_uid: u32) -> Result<Vec<u32>> {
    let query = format!("UID {}:*", since_uid + 1);
    let uid_set = client
        .session_mut()
        .uid_search(&query)
        .await
        .map_err(|e| anyhow::anyhow!("UID SEARCH {query}: {e}"))?;

    let mut uids: Vec<u32> = uid_set.into_iter().collect();
    uids.sort_unstable();
    Ok(uids)
}
