// Copyright (c) 2026 Tyler Martin
// Licensed under FSL-1.1-ALv2 (see LICENSE)

pub mod discovery;
pub mod errors;
pub mod folders;
pub mod imap;
pub mod smtp;

pub use discovery::{discover, DiscoveryCandidate};
pub use errors::{DiscoveryError, ImapError, SmtpError};
pub use folders::{detect_drafts_folder, detect_sent_folder};
pub use imap::ImapClient;
pub use smtp::SmtpSender;
