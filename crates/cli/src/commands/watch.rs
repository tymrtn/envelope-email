// Copyright (c) 2026 Tyler Martin
// Licensed under FSL-1.1-ALv2 (see LICENSE)

use std::time::Duration;

use anyhow::{Context, Result};
use async_imap::extensions::idle::IdleResponse;
use envelope_email_store::models::Event;
use envelope_email_store::CredentialBackend;
use futures_util::StreamExt;
use tracing::{info, warn};

use super::common::setup_credentials;

#[tokio::main]
pub async fn run(
    folder: &str,
    account: Option<&str>,
    webhook: Option<&str>,
    _run_rules: bool,
    json: bool,
    backend: CredentialBackend,
) -> Result<()> {
    let (db, creds) = setup_credentials(account, backend)?;
    let account_id = creds.account.id.clone();

    let http_client = webhook.map(|_| reqwest::Client::new());

    if !json {
        eprintln!(
            "Watching {} on {}... (Ctrl-C to stop)",
            folder, creds.account.username
        );
    }

    // Graceful shutdown via Ctrl-C
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);

    let mut session = envelope_email_transport::idle::connect_session(&creds)
        .await
        .context("IMAP connection failed")?;

    session
        .select(folder)
        .await
        .map_err(|e| anyhow::anyhow!("SELECT {folder}: {e}"))?;

    // Track highest UID we've seen so we only fetch genuinely new messages
    let mut last_uid: u32 = highest_uid(&mut session, folder).await.unwrap_or(0);

    loop {
        // Enter IDLE
        let mut handle = session.idle();
        handle.init().await.map_err(|e| anyhow::anyhow!("IDLE init: {e}"))?;

        let (idle_fut, _interrupt) =
            handle.wait_with_timeout(Duration::from_secs(25 * 60));

        let response = idle_fut.await.map_err(|e| anyhow::anyhow!("IDLE wait: {e}"))?;

        match response {
            IdleResponse::NewData(_data) => {
                // End IDLE to regain session ownership
                session = handle
                    .done()
                    .await
                    .map_err(|e| anyhow::anyhow!("IDLE done: {e}"))?;

                // Re-SELECT to refresh EXISTS
                session
                    .select(folder)
                    .await
                    .map_err(|e| anyhow::anyhow!("SELECT {folder}: {e}"))?;

                // Fetch messages newer than our watermark
                let new_msgs = fetch_new_messages(&mut session, last_uid).await?;

                for msg in &new_msgs {
                    let uid = msg.uid;
                    if uid > last_uid {
                        last_uid = uid;
                    }

                    let event = Event {
                        id: uuid::Uuid::new_v4().to_string(),
                        account_id: account_id.clone(),
                        event_type: "new_message".to_string(),
                        folder: folder.to_string(),
                        uid: Some(i64::from(uid)),
                        message_id: msg.message_id.clone(),
                        from_addr: msg.from_addr.clone(),
                        subject: msg.subject.clone(),
                        snippet: msg.snippet.clone(),
                        payload: None,
                        created_at: chrono::Utc::now()
                            .format("%Y-%m-%dT%H:%M:%S")
                            .to_string(),
                    };

                    // Persist to SQLite
                    if let Err(e) = db.insert_event(&event) {
                        warn!("failed to persist event: {e}");
                    }

                    // Emit JSON line to stdout
                    let json_line = serde_json::to_string(&event)
                        .unwrap_or_else(|_| "{}".to_string());
                    println!("{json_line}");

                    // Webhook delivery (fire-and-forget)
                    if let (Some(url), Some(client)) = (webhook, http_client.as_ref()) {
                        let url = url.to_string();
                        let client = client.clone();
                        let body = json_line.clone();
                        tokio::spawn(async move {
                            if let Err(e) = client
                                .post(&url)
                                .header("Content-Type", "application/json")
                                .body(body)
                                .send()
                                .await
                            {
                                warn!("webhook POST failed: {e}");
                            }
                        });
                    }
                }

                info!("processed {} new message(s)", new_msgs.len());
            }
            IdleResponse::Timeout => {
                // Re-IDLE after timeout (keeps connection alive)
                session = handle
                    .done()
                    .await
                    .map_err(|e| anyhow::anyhow!("IDLE done after timeout: {e}"))?;

                // Re-SELECT to keep the mailbox session alive
                session
                    .select(folder)
                    .await
                    .map_err(|e| anyhow::anyhow!("SELECT {folder}: {e}"))?;
            }
            IdleResponse::ManualInterrupt => {
                let _ = handle.done().await;
                break;
            }
        }

        // Check if Ctrl-C was pressed
        if futures_util::FutureExt::now_or_never(&mut shutdown).is_some() {
            if !json {
                eprintln!("Shutting down...");
            }
            break;
        }
    }

    Ok(())
}

/// A minimal representation of a newly fetched message.
struct NewMessage {
    uid: u32,
    message_id: Option<String>,
    from_addr: Option<String>,
    subject: Option<String>,
    snippet: Option<String>,
}

/// Return the highest UID currently in the selected folder.
async fn highest_uid(
    session: &mut envelope_email_transport::imap::ImapSession,
    _folder: &str,
) -> Result<u32> {
    // SEARCH for all messages to find max UID
    let uids = session
        .uid_search("ALL")
        .await
        .map_err(|e| anyhow::anyhow!("UID SEARCH ALL: {e}"))?;
    Ok(uids.into_iter().max().unwrap_or(0))
}

/// Fetch messages with UID > last_uid from the already-selected folder.
async fn fetch_new_messages(
    session: &mut envelope_email_transport::imap::ImapSession,
    last_uid: u32,
) -> Result<Vec<NewMessage>> {
    let start = last_uid + 1;
    let range = format!("{start}:*");

    let fetches = session
        .uid_fetch(&range, "(UID ENVELOPE BODY.PEEK[TEXT]<0.200>)")
        .await
        .map_err(|e| anyhow::anyhow!("UID FETCH {range}: {e}"))?;

    let mut messages = Vec::new();
    let mut stream = fetches;
    while let Some(item) = stream.next().await {
        match item {
            Ok(fetch) => {
                let uid = fetch.uid.unwrap_or(0);
                if uid <= last_uid {
                    // UID FETCH N:* always returns at least UID N even if
                    // there are no new messages.
                    continue;
                }

                let (message_id, from_addr, subject) = if let Some(env) = fetch.envelope() {
                    let mid = env
                        .message_id
                        .as_ref()
                        .map(|m| String::from_utf8_lossy(m).to_string());
                    let from = env.from.as_ref().and_then(|addrs| {
                        addrs.first().map(|a| {
                            let mailbox = a
                                .mailbox
                                .as_ref()
                                .map(|m| String::from_utf8_lossy(m).to_string())
                                .unwrap_or_default();
                            let host = a
                                .host
                                .as_ref()
                                .map(|h| String::from_utf8_lossy(h).to_string())
                                .unwrap_or_default();
                            format!("{mailbox}@{host}")
                        })
                    });
                    let subj = env
                        .subject
                        .as_ref()
                        .map(|s| String::from_utf8_lossy(s).to_string());
                    (mid, from, subj)
                } else {
                    (None, None, None)
                };

                let snippet = fetch.text().map(|t| {
                    let s = String::from_utf8_lossy(t);
                    if s.len() > 150 {
                        format!("{}...", &s[..150])
                    } else {
                        s.to_string()
                    }
                });

                messages.push(NewMessage {
                    uid,
                    message_id,
                    from_addr,
                    subject,
                    snippet,
                });
            }
            Err(e) => {
                warn!("FETCH parse error (skipping): {e}");
            }
        }
    }

    Ok(messages)
}
