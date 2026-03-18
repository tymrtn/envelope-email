// Copyright (c) 2026 Tyler Martin
// Licensed under FSL-1.1-ALv2 (see LICENSE)

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub username: String,
    pub domain: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_username: Option<String>,
    pub imap_username: Option<String>,
    pub display_name: Option<String>,
    pub signature_text: Option<String>,
    pub signature_html: Option<String>,
    pub created_at: String,
}

/// Account with decrypted credentials — never serialize to JSON output.
pub struct AccountWithCredentials {
    pub account: Account,
    pub password: String,
    pub smtp_password: Option<String>,
    pub imap_password: Option<String>,
}

impl AccountWithCredentials {
    pub fn effective_smtp_username(&self) -> &str {
        self.account
            .smtp_username
            .as_deref()
            .unwrap_or(&self.account.username)
    }

    pub fn effective_smtp_password(&self) -> &str {
        self.smtp_password.as_deref().unwrap_or(&self.password)
    }

    pub fn effective_imap_username(&self) -> &str {
        self.account
            .imap_username
            .as_deref()
            .unwrap_or(&self.account.username)
    }

    pub fn effective_imap_password(&self) -> &str {
        self.imap_password.as_deref().unwrap_or(&self.password)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Draft {
    pub id: String,
    pub account_id: String,
    pub status: DraftStatus,
    pub to_addr: String,
    pub cc_addr: Option<String>,
    pub bcc_addr: Option<String>,
    pub reply_to: Option<String>,
    pub subject: Option<String>,
    pub text_content: Option<String>,
    pub html_content: Option<String>,
    pub in_reply_to: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub attachments: Vec<serde_json::Value>,
    pub message_id: Option<String>,
    pub send_after: Option<String>,
    pub snoozed_until: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub sent_at: Option<String>,
    pub created_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DraftStatus {
    Draft,
    PendingReview,
    Blocked,
    Sent,
    Discarded,
}

impl DraftStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Draft => "draft",
            Self::PendingReview => "pending_review",
            Self::Blocked => "blocked",
            Self::Sent => "sent",
            Self::Discarded => "discarded",
        }
    }

    pub fn is_editable(&self) -> bool {
        matches!(self, Self::Draft | Self::PendingReview | Self::Blocked)
    }
}

impl std::str::FromStr for DraftStatus {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "draft" => Ok(Self::Draft),
            "pending_review" => Ok(Self::PendingReview),
            "blocked" => Ok(Self::Blocked),
            "sent" => Ok(Self::Sent),
            "discarded" => Ok(Self::Discarded),
            _ => Err(format!("unknown draft status: {s}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionLog {
    pub id: String,
    pub account_id: String,
    pub action_type: String,
    pub confidence: f64,
    pub justification: String,
    pub action_taken: String,
    pub message_id: Option<String>,
    pub draft_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredLicense {
    pub id: String,
    pub token: String,
    pub licensee: String,
    pub expires_at: String,
    pub features: Vec<String>,
    pub activated_at: String,
}

/// Summary of an IMAP message (envelope data, no body).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSummary {
    pub uid: u32,
    pub message_id: Option<String>,
    pub from_addr: String,
    pub to_addr: String,
    pub subject: String,
    pub date: Option<String>,
    pub flags: Vec<String>,
    pub size: u32,
}

/// Full IMAP message with body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub uid: u32,
    pub message_id: Option<String>,
    pub from_addr: String,
    pub to_addr: String,
    pub cc_addr: Option<String>,
    pub subject: String,
    pub date: Option<String>,
    pub text_body: Option<String>,
    pub html_body: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    pub flags: Vec<String>,
    pub attachments: Vec<AttachmentMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentMeta {
    pub filename: String,
    pub content_type: String,
    pub size: u64,
    pub content_id: Option<String>,
}

/// Discovery result for IMAP/SMTP auto-configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryResult {
    pub domain: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_source: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_source: String,
}
