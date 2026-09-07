// Copyright (c) 2026 Tyler Martin
// Licensed under FSL-1.1-ALv2 (see LICENSE)

//! `envelope code` — poll IMAP for a verification code and extract it.

use anyhow::{Context, Result, bail};
use envelope_email_store::credential_store::CredentialBackend;
use envelope_email_transport::code_extractor::extract_code;
use envelope_email_transport::imap;

use super::common::setup_credentials;
use super::provenance;

const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
/// JSON is the supported unattended/agent surface. Collect for one extra poll
/// interval so a forged first arrival cannot win before legitimate candidates.
const AUTOMATION_STABILIZATION_WINDOW: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq)]
struct OtpCandidate {
    code: String,
    from: String,
    subject: String,
}

#[derive(Debug, PartialEq, Eq)]
enum CollectionOutcome {
    Continue,
    Ready(OtpCandidate),
    Ambiguous(usize),
}

#[derive(Default)]
struct CandidateCollection {
    candidates: Vec<OtpCandidate>,
    first_seen_at: Option<std::time::Duration>,
}

impl CandidateCollection {
    fn observe(
        &mut self,
        now: std::time::Duration,
        matches: impl IntoIterator<Item = OtpCandidate>,
    ) -> CollectionOutcome {
        self.candidates.extend(matches);
        if self.candidates.len() > 1 {
            return CollectionOutcome::Ambiguous(self.candidates.len());
        }

        let Some(candidate) = self.candidates.first().cloned() else {
            return CollectionOutcome::Continue;
        };
        let first_seen_at = *self.first_seen_at.get_or_insert(now);
        if now.saturating_sub(first_seen_at) >= AUTOMATION_STABILIZATION_WINDOW {
            CollectionOutcome::Ready(candidate)
        } else {
            CollectionOutcome::Continue
        }
    }
}

/// `envelope code` — poll IMAP for new messages and extract a verification code.
///
/// The JSON surface is unattended/agent automation. It requires a caller-selected
/// account and a narrow exact mailbox or full-domain sender filter, then collects
/// candidates across a bounded stabilization window. The `From` header remains
/// untrusted message content; Envelope does not claim it is authenticated.
#[tokio::main]
pub async fn run(
    account: Option<&str>,
    from_filter: Option<&str>,
    subject_filter: Option<&str>,
    wait_secs: u64,
    json: bool,
    backend: CredentialBackend,
) -> Result<()> {
    if json {
        if let Some(error) = automation_binding_error(account, from_filter) {
            println!(
                "{}",
                serde_json::json!({
                    "error": "automation_binding_required",
                    "reason": error,
                    "trust": provenance::inbound_trust(),
                })
            );
            bail!(
                "OTP JSON automation requires an explicit --account and exact --from address or full domain"
            );
        }
    }

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
    let mut collected = CandidateCollection::default();
    loop {
        let elapsed = start.elapsed();
        if elapsed >= timeout {
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
                matches.push(OtpCandidate {
                    code,
                    from: msg.from_addr,
                    subject: msg.subject,
                });
            }
        }

        if json {
            match collected.observe(elapsed, matches) {
                CollectionOutcome::Continue => {}
                CollectionOutcome::Ready(candidate) => {
                    let output = provenance::annotate_inbound(serde_json::json!({
                        "code": candidate.code,
                        "from": candidate.from,
                        "subject": candidate.subject,
                    }));
                    println!("{}", serde_json::to_string_pretty(&output)?);
                    return Ok(());
                }
                CollectionOutcome::Ambiguous(candidate_count) => {
                    println!(
                        "{}",
                        serde_json::json!({
                            "error": "ambiguous_matches",
                            "candidate_count": candidate_count,
                            "trust": provenance::inbound_trust(),
                        })
                    );
                    bail!(
                        "ambiguous OTP matches: {candidate_count} messages matched; refine --from or --subject"
                    );
                }
            }
        } else {
            // Interactive stdout use remains low-friction and intentionally does
            // not imply the collection/authentication guarantees of --json.
            match matches.as_slice() {
                [] => {}
                [candidate] => {
                    println!("{}", candidate.code);
                    return Ok(());
                }
                _ => bail!(
                    "ambiguous OTP matches: {} messages matched this poll; refine --from or --subject",
                    matches.len()
                ),
            }
        }

        let remaining = timeout.saturating_sub(start.elapsed());
        tokio::time::sleep(POLL_INTERVAL.min(remaining)).await;
    }
}

/// Return the reason JSON automation is not safely bound, if any. This runs
/// before credentials are opened or any network connection is attempted.
fn automation_binding_error(
    account: Option<&str>,
    from_filter: Option<&str>,
) -> Option<&'static str> {
    if account.is_none_or(|value| value.trim().is_empty()) {
        return Some("--account is required for JSON OTP automation");
    }
    if !from_filter.is_some_and(is_narrow_sender_filter) {
        return Some(
            "--from must be an exact mailbox address or full domain for JSON OTP automation",
        );
    }
    None
}

/// JSON automation accepts only an exact address or a fully-qualified domain;
/// display names, local fragments, wildcards, and sender substrings are broad.
fn is_narrow_sender_filter(raw_filter: &str) -> bool {
    let filter = raw_filter
        .trim()
        .trim_matches('<')
        .trim_matches('>')
        .to_lowercase();
    if filter.is_empty() || filter.contains(char::is_whitespace) {
        return false;
    }

    if let Some((local, domain)) = filter.rsplit_once('@') {
        return !local.is_empty() && !local.contains('@') && is_fully_qualified_domain(domain);
    }
    is_fully_qualified_domain(filter.trim_start_matches('@'))
}

fn is_fully_qualified_domain(domain: &str) -> bool {
    let labels: Vec<_> = domain.split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
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

    fn candidate(code: &str) -> OtpCandidate {
        OtpCandidate {
            code: code.to_string(),
            from: "otp@issuer.example".to_string(),
            subject: "Your verification code".to_string(),
        }
    }

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

    #[test]
    fn json_automation_rejects_empty_or_broad_bindings() {
        assert!(automation_binding_error(None, Some("issuer.example")).is_some());
        assert!(automation_binding_error(Some(""), Some("issuer.example")).is_some());
        assert!(automation_binding_error(Some("account-1"), None).is_some());
        assert!(automation_binding_error(Some("account-1"), Some("")).is_some());
        assert!(automation_binding_error(Some("account-1"), Some("issuer")).is_some());
        assert!(automation_binding_error(Some("account-1"), Some("otp@*.example")).is_some());
    }

    #[test]
    fn json_automation_accepts_exact_address_or_full_domain_with_account() {
        assert!(automation_binding_error(Some("account-1"), Some("otp@issuer.example")).is_none());
        assert!(automation_binding_error(Some("account-1"), Some("issuer.example")).is_none());
    }

    #[test]
    fn json_collection_waits_for_stabilization_across_polls() {
        let mut collection = CandidateCollection::default();
        assert_eq!(
            collection.observe(std::time::Duration::ZERO, [candidate("111111")]),
            CollectionOutcome::Continue
        );
        assert_eq!(
            collection.observe(std::time::Duration::from_secs(4), []),
            CollectionOutcome::Continue
        );
        assert_eq!(
            collection.observe(std::time::Duration::from_secs(5), []),
            CollectionOutcome::Ready(candidate("111111"))
        );
    }

    #[test]
    fn json_collection_fails_closed_for_candidates_from_separate_polls() {
        let mut collection = CandidateCollection::default();
        assert_eq!(
            collection.observe(std::time::Duration::ZERO, [candidate("111111")]),
            CollectionOutcome::Continue
        );
        assert_eq!(
            collection.observe(std::time::Duration::from_secs(5), [candidate("222222")]),
            CollectionOutcome::Ambiguous(2)
        );
    }

    #[test]
    fn json_collection_fails_closed_for_multiple_candidates_in_one_poll() {
        let mut collection = CandidateCollection::default();
        assert_eq!(
            collection.observe(
                std::time::Duration::ZERO,
                [candidate("111111"), candidate("222222")]
            ),
            CollectionOutcome::Ambiguous(2)
        );
    }

    #[test]
    fn timeout_before_stabilization_never_returns_a_candidate() {
        let mut collection = CandidateCollection::default();
        assert_eq!(
            collection.observe(std::time::Duration::ZERO, [candidate("111111")]),
            CollectionOutcome::Continue
        );
        // The run loop checks this timeout before another poll, so a 4-second
        // request cannot release a candidate that requires five seconds to stabilize.
        assert!(std::time::Duration::from_secs(4) < AUTOMATION_STABILIZATION_WINDOW);
    }
}
