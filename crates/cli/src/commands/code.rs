// Copyright (c) 2026 Tyler Martin
// Licensed under FSL-1.1-ALv2 (see LICENSE)

//! `envelope code` — poll IMAP for a verification code and extract it.

use anyhow::{Context, Result, bail};
use envelope_email_store::credential_store::CredentialBackend;
use envelope_email_transport::code_extractor::extract_code;
use envelope_email_transport::imap;

use super::common::setup_credentials;
use super::provenance;

/// `envelope code` — poll IMAP for new messages and extract a verification code.
///
/// Sender filters are exact mailbox/domain identity filters. When a polling
/// batch has multiple matching codes we fail closed rather than picking an
/// arrival-order winner for automation.
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
    let mut last_seen_uid = initial_max_uid;
    loop {
        if start.elapsed() >= timeout {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"error": "timeout", "waited_seconds": wait_secs, "trust": provenance::inbound_trust()})
                );
            } else {
                eprintln!("Timeout: no verification code found after {wait_secs}s");
            }
            std::process::exit(1);
        }

        let new_uids = search_new_uids(&mut client, last_seen_uid).await?;
        let mut matches = Vec::new();
        for uid in new_uids {
            if uid <= last_seen_uid {
                continue;
            }
            last_seen_uid = uid;
            let msg = match imap::fetch_message(&mut client, "INBOX", uid).await? {
                Some(m) => m,
                None => continue,
            };
            if from_filter.is_some_and(|filter| !sender_matches(&msg.from_addr, filter)) {
                continue;
            }
            if let Some(subject_filter) = subject_filter {
                if !msg
                    .subject
                    .to_lowercase()
                    .contains(&subject_filter.to_lowercase())
                {
                    continue;
                }
            }
            if let Some(code) = extract_code(
                msg.text_body.as_deref().unwrap_or(""),
                msg.html_body.as_deref(),
            ) {
                matches.push((code, msg.from_addr, msg.subject));
            }
        }

        match matches.as_slice() {
            [] => {}
            [(code, from, subject)] => {
                if json {
                    let output = provenance::annotate_inbound(serde_json::json!({
                        "code": code, "from": from, "subject": subject,
                    }));
                    println!("{}", serde_json::to_string_pretty(&output)?);
                } else {
                    println!("{code}");
                }
                return Ok(());
            }
            _ => {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "error": "ambiguous_matches",
                            "candidate_count": matches.len(),
                            "trust": provenance::inbound_trust(),
                        })
                    );
                }
                bail!(
                    "ambiguous OTP matches: {} messages matched this poll; refine --from or --subject",
                    matches.len()
                );
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

/// Extract a mailbox from a header and compare exact case-insensitive mailbox
/// identity or a whole domain. Display names are never candidates.
fn sender_matches(raw_sender: &str, raw_filter: &str) -> bool {
    let sender = mailbox_from_header(raw_sender);
    let filter = raw_filter
        .trim()
        .trim_matches('<')
        .trim_matches('>')
        .to_lowercase();
    if sender.is_empty() || filter.is_empty() {
        return false;
    }
    if filter.contains('@') && !filter.starts_with('@') {
        return sender == filter;
    }
    let domain = filter.trim_start_matches('@');
    !domain.is_empty()
        && sender
            .rsplit_once('@')
            .is_some_and(|(_, value)| value == domain)
}

fn mailbox_from_header(raw: &str) -> String {
    let candidate = raw
        .rsplit_once('<')
        .and_then(|(_, rest)| rest.split_once('>').map(|(mailbox, _)| mailbox))
        .unwrap_or(raw)
        .trim()
        .to_lowercase();
    (candidate.matches('@').count() == 1 && !candidate.contains(char::is_whitespace))
        .then_some(candidate)
        .unwrap_or_default()
}

async fn get_max_uid(client: &mut imap::ImapClient) -> Result<u32> {
    let uid_set = client
        .session_mut()
        .uid_search("ALL")
        .await
        .map_err(|e| anyhow::anyhow!("UID SEARCH ALL: {e}"))?;
    Ok(uid_set.into_iter().max().unwrap_or(0))
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sender_filter_is_exact_address_or_exact_domain() {
        assert!(sender_matches(
            "Trusted Name <otp@issuer.example>",
            "otp@issuer.example"
        ));
        assert!(sender_matches("otp@issuer.example", "issuer.example"));
        assert!(!sender_matches(
            "attacker@issuer.example.evil",
            "issuer.example"
        ));
        assert!(!sender_matches(
            "issuer.example <attacker@evil.test>",
            "issuer.example"
        ));
        assert!(!sender_matches("otp@issuer.example", "issuer"));
    }
}
