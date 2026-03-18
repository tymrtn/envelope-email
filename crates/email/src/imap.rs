// Copyright (c) 2026 Tyler Martin
// Licensed under FSL-1.1-ALv2 (see LICENSE)

use std::sync::Arc;

use async_imap::Session;
use envelope_email_store::models::{AccountWithCredentials, AttachmentMeta, Message, MessageSummary};
use futures_util::StreamExt;
use mail_parser::MimeHeaders;
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;
use tracing::{debug, info};

use crate::errors::ImapError;

type ImapSession = Session<TlsStream<TcpStream>>;

/// IMAP client wrapping an authenticated async-imap session.
pub struct ImapClient {
    session: ImapSession,
}

impl ImapClient {
    pub fn session_mut(&mut self) -> &mut ImapSession {
        &mut self.session
    }
}

/// Connect to an IMAP server over TLS and authenticate.
pub async fn connect(account: &AccountWithCredentials) -> Result<ImapClient, ImapError> {
    let host = &account.account.imap_host;
    let port = account.account.imap_port;
    let username = account.effective_imap_username();
    let password = account.effective_imap_password();

    info!("connecting to IMAP {host}:{port} as {username}");

    let tcp = TcpStream::connect((host.as_str(), port))
        .await
        .map_err(|e| ImapError::Connection(format!("{host}:{port}: {e}")))?;

    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls_config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    let connector = TlsConnector::from(Arc::new(tls_config));
    let server_name = rustls::pki_types::ServerName::try_from(host.as_str())
        .map_err(|e| ImapError::Connection(format!("invalid server name {host}: {e}")))?
        .to_owned();

    let tls_stream = connector
        .connect(server_name, tcp)
        .await
        .map_err(|e| ImapError::Connection(format!("TLS handshake with {host}: {e}")))?;

    let client = async_imap::Client::new(tls_stream);

    let session = client
        .login(username, password)
        .await
        .map_err(|(e, _)| ImapError::Auth(format!("login failed for {username}@{host}: {e}")))?;

    debug!("IMAP session established for {username}@{host}");
    Ok(ImapClient { session })
}

/// List all mailbox folders.
pub async fn list_folders(client: &mut ImapClient) -> Result<Vec<String>, ImapError> {
    let mailboxes = client
        .session
        .list(Some(""), Some("*"))
        .await
        .map_err(|e| ImapError::Protocol(format!("LIST command failed: {e}")))?;

    let mut folders = Vec::new();
    let mut stream = mailboxes;
    while let Some(item) = stream.next().await {
        match item {
            Ok(mailbox) => folders.push(mailbox.name().to_string()),
            Err(e) => return Err(ImapError::Protocol(format!("LIST parse error: {e}"))),
        }
    }

    debug!("listed {} folders", folders.len());
    Ok(folders)
}

/// Fetch message summaries from a folder.
pub async fn fetch_inbox(
    client: &mut ImapClient,
    folder: &str,
    limit: u32,
) -> Result<Vec<MessageSummary>, ImapError> {
    let mailbox = client
        .session
        .select(folder)
        .await
        .map_err(|e| ImapError::Protocol(format!("SELECT {folder}: {e}")))?;

    let exists = mailbox.exists;
    if exists == 0 {
        return Ok(Vec::new());
    }

    let start = if exists > limit { exists - limit + 1 } else { 1 };
    let range = format!("{start}:{exists}");

    let messages = client
        .session
        .fetch(&range, "(UID FLAGS ENVELOPE RFC822.SIZE)")
        .await
        .map_err(|e| ImapError::Protocol(format!("FETCH {range}: {e}")))?;

    let mut summaries = Vec::new();
    let mut stream = messages;
    while let Some(item) = stream.next().await {
        match item {
            Ok(fetch) => {
                let uid = fetch.uid.unwrap_or(0);
                let flags: Vec<String> = fetch
                    .flags()
                    .map(|f| format!("{f:?}"))
                    .collect();
                let size = fetch.size.unwrap_or(0);

                let (from_addr, to_addr, subject, date, message_id) =
                    if let Some(env) = fetch.envelope() {
                        let from = imap_envelope_addresses(&env.from);
                        let to = imap_envelope_addresses(&env.to);
                        let subj = env
                            .subject
                            .as_ref()
                            .map(|s| String::from_utf8_lossy(s).to_string())
                            .unwrap_or_default();
                        let dt = env
                            .date
                            .as_ref()
                            .map(|d| String::from_utf8_lossy(d).to_string());
                        let mid = env
                            .message_id
                            .as_ref()
                            .map(|m| String::from_utf8_lossy(m).to_string());
                        (from, to, subj, dt, mid)
                    } else {
                        (String::new(), String::new(), String::new(), None, None)
                    };

                summaries.push(MessageSummary {
                    uid,
                    message_id,
                    from_addr,
                    to_addr,
                    subject,
                    date,
                    flags,
                    size,
                });
            }
            Err(e) => return Err(ImapError::Protocol(format!("FETCH parse error: {e}"))),
        }
    }

    Ok(summaries)
}

/// Fetch a full message by UID, parsing the body with mail-parser.
pub async fn fetch_message(
    client: &mut ImapClient,
    folder: &str,
    uid: u32,
) -> Result<Option<Message>, ImapError> {
    client
        .session
        .select(folder)
        .await
        .map_err(|e| ImapError::Protocol(format!("SELECT {folder}: {e}")))?;

    let uid_range = format!("{uid}");
    let messages = client
        .session
        .uid_fetch(&uid_range, "(UID FLAGS BODY.PEEK[])")
        .await
        .map_err(|e| ImapError::Protocol(format!("UID FETCH {uid}: {e}")))?;

    let mut stream = messages;
    while let Some(item) = stream.next().await {
        match item {
            Ok(fetch) => {
                let body: &[u8] = fetch.body().unwrap_or_default();
                let parsed = mail_parser::MessageParser::default().parse(body);

                if let Some(parsed) = parsed {
                    let flags: Vec<String> = fetch
                        .flags()
                        .map(|f| format!("{f:?}"))
                        .collect();

                    let from_addr = mp_first_address(parsed.from());
                    let to_addr = mp_first_address(parsed.to());
                    let cc_addr = {
                        let addr = mp_first_address(parsed.cc());
                        if addr.is_empty() { None } else { Some(addr) }
                    };

                    let subject = parsed.subject().unwrap_or_default().to_string();
                    let date = parsed.date().map(|d| d.to_rfc3339());

                    let text_body = parsed.body_text(0).map(|t| t.to_string());
                    let html_body = parsed.body_html(0).map(|h| h.to_string());

                    let in_reply_to = parsed.in_reply_to().as_text().map(|s| s.to_string());
                    let references = parsed.references().as_text().map(|s| s.to_string());
                    let message_id = parsed.message_id().map(|s| s.to_string());

                    let attachments: Vec<AttachmentMeta> = parsed
                        .attachments()
                        .map(|a| {
                            let ct: Option<&mail_parser::ContentType> = a.content_type();
                            AttachmentMeta {
                                filename: a
                                    .attachment_name()
                                    .unwrap_or("unnamed")
                                    .to_string(),
                                content_type: ct
                                    .map(|ct| {
                                        let subtype = ct.subtype().unwrap_or("octet-stream");
                                        format!("{}/{subtype}", ct.ctype())
                                    })
                                    .unwrap_or_else(|| "application/octet-stream".to_string()),
                                size: a.len() as u64,
                                content_id: a.content_id().map(|s: &str| s.to_string()),
                            }
                        })
                        .collect();

                    return Ok(Some(Message {
                        uid,
                        message_id,
                        from_addr,
                        to_addr,
                        cc_addr,
                        subject,
                        date,
                        text_body,
                        html_body,
                        in_reply_to,
                        references,
                        flags,
                        attachments,
                    }));
                } else {
                    return Ok(None);
                }
            }
            Err(e) => return Err(ImapError::Protocol(format!("UID FETCH parse error: {e}"))),
        }
    }

    Ok(None)
}

/// Search messages in a folder using IMAP SEARCH.
pub async fn search(
    _client: &mut ImapClient,
    _folder: &str,
    _query: &str,
    _limit: u32,
) -> Result<Vec<MessageSummary>, ImapError> {
    Err(ImapError::Connection("search not yet implemented".into()))
}

pub async fn move_message(
    _client: &mut ImapClient,
    _uid: u32,
    _from: &str,
    _to: &str,
) -> Result<(), ImapError> {
    Err(ImapError::Connection("move_message not yet implemented".into()))
}

pub async fn copy_message(
    _client: &mut ImapClient,
    _uid: u32,
    _from: &str,
    _to: &str,
) -> Result<(), ImapError> {
    Err(ImapError::Connection("copy_message not yet implemented".into()))
}

pub async fn delete_message(
    _client: &mut ImapClient,
    _folder: &str,
    _uid: u32,
) -> Result<(), ImapError> {
    Err(ImapError::Connection("delete_message not yet implemented".into()))
}

pub async fn set_flag(
    _client: &mut ImapClient,
    _folder: &str,
    _uid: u32,
    _flag: &str,
) -> Result<(), ImapError> {
    Err(ImapError::Connection("set_flag not yet implemented".into()))
}

pub async fn remove_flag(
    _client: &mut ImapClient,
    _folder: &str,
    _uid: u32,
    _flag: &str,
) -> Result<(), ImapError> {
    Err(ImapError::Connection("remove_flag not yet implemented".into()))
}

/// Extract first email address from a mail-parser Address.
fn mp_first_address(header: Option<&mail_parser::Address<'_>>) -> String {
    match header {
        Some(addr) => match addr {
            mail_parser::Address::List(list) => list
                .first()
                .and_then(|a| a.address.as_ref())
                .map(|a| a.to_string())
                .unwrap_or_default(),
            mail_parser::Address::Group(groups) => groups
                .first()
                .and_then(|g| g.addresses.first())
                .and_then(|a| a.address.as_ref())
                .map(|a| a.to_string())
                .unwrap_or_default(),
        },
        None => String::new(),
    }
}

/// Format IMAP envelope addresses into a comma-separated string.
fn imap_envelope_addresses(
    addrs: &Option<Vec<imap_proto::types::Address<'_>>>,
) -> String {
    match addrs {
        Some(list) => list
            .iter()
            .map(|a| {
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
                if host.is_empty() {
                    mailbox
                } else {
                    format!("{mailbox}@{host}")
                }
            })
            .collect::<Vec<_>>()
            .join(", "),
        None => String::new(),
    }
}
