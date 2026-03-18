// Copyright (c) 2026 Tyler Martin
// Licensed under FSL-1.1-ALv2 (see LICENSE)

mod commands;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "envelope-email", about = "BYO mailbox email client")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Output as JSON
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage email accounts
    Accounts {
        #[command(subcommand)]
        subcommand: AccountsCmd,
    },

    /// List messages in a folder
    Inbox {
        /// IMAP folder to list
        #[arg(long, default_value = "INBOX")]
        folder: String,
        /// Maximum messages to return
        #[arg(long, default_value = "25")]
        limit: u32,
        /// Account ID or email
        #[arg(long)]
        account: Option<String>,
    },

    /// Read a single message by UID
    Read {
        /// Message UID
        uid: u32,
        /// IMAP folder
        #[arg(long, default_value = "INBOX")]
        folder: String,
        /// Account ID or email
        #[arg(long)]
        account: Option<String>,
    },

    /// Search messages
    Search {
        /// IMAP search query
        query: String,
        /// IMAP folder
        #[arg(long, default_value = "INBOX")]
        folder: String,
        /// Maximum results
        #[arg(long, default_value = "25")]
        limit: u32,
        /// Account ID or email
        #[arg(long)]
        account: Option<String>,
    },

    /// Send an email
    Send {
        /// Recipient address
        #[arg(long)]
        to: String,
        /// Subject line
        #[arg(long)]
        subject: String,
        /// Plain-text body
        #[arg(long)]
        body: Option<String>,
        /// HTML body
        #[arg(long)]
        html: Option<String>,
        /// CC addresses (comma-separated)
        #[arg(long)]
        cc: Option<String>,
        /// BCC addresses (comma-separated)
        #[arg(long)]
        bcc: Option<String>,
        /// Reply-To address
        #[arg(long)]
        reply_to: Option<String>,
        /// Account ID or email
        #[arg(long)]
        account: Option<String>,
    },

    /// Move a message to another folder
    Move {
        /// Message UID
        uid: u32,
        /// Destination folder
        #[arg(long)]
        to_folder: String,
        /// Source folder
        #[arg(long, default_value = "INBOX")]
        folder: String,
        /// Account ID or email
        #[arg(long)]
        account: Option<String>,
    },

    /// Copy a message to another folder
    Copy {
        /// Message UID
        uid: u32,
        /// Destination folder
        #[arg(long)]
        to_folder: String,
        /// Source folder
        #[arg(long, default_value = "INBOX")]
        folder: String,
        /// Account ID or email
        #[arg(long)]
        account: Option<String>,
    },

    /// Delete a message
    Delete {
        /// Message UID
        uid: u32,
        /// IMAP folder
        #[arg(long, default_value = "INBOX")]
        folder: String,
        /// Account ID or email
        #[arg(long)]
        account: Option<String>,
    },

    /// Manage message flags
    Flag {
        #[command(subcommand)]
        subcommand: FlagCmd,
    },

    /// List IMAP folders
    Folders {
        /// Account ID or email
        #[arg(long)]
        account: Option<String>,
    },

    /// Manage attachments
    Attachment {
        #[command(subcommand)]
        subcommand: AttachmentCmd,
    },

    /// Manage drafts
    Draft {
        #[command(subcommand)]
        subcommand: DraftCmd,
    },

    /// Start the localhost dashboard
    Serve {
        /// Port to listen on
        #[arg(long, default_value = "3141")]
        port: u16,
    },

    /// Compose a new email (licensed tier)
    Compose {
        /// Account ID or email
        #[arg(long)]
        account: Option<String>,
    },

    /// Manage license activation
    License {
        #[command(subcommand)]
        subcommand: LicenseCmd,
    },

    /// Show account attributes
    Attributes {
        /// Account ID or email
        #[arg(long)]
        account: Option<String>,
    },

    /// View action log
    Actions {
        #[command(subcommand)]
        subcommand: ActionsCmd,
    },
}

#[derive(Subcommand)]
enum AccountsCmd {
    /// Add a new email account
    Add {
        /// Email address
        #[arg(long)]
        email: String,
        /// Password (will prompt if not given)
        #[arg(long)]
        password: Option<String>,
        /// Account display name
        #[arg(long)]
        name: Option<String>,
        /// SMTP host (auto-discovered if omitted)
        #[arg(long)]
        smtp_host: Option<String>,
        /// SMTP port
        #[arg(long)]
        smtp_port: Option<u16>,
        /// IMAP host (auto-discovered if omitted)
        #[arg(long)]
        imap_host: Option<String>,
        /// IMAP port
        #[arg(long)]
        imap_port: Option<u16>,
    },
    /// List configured accounts
    List,
    /// Remove an account
    Remove {
        /// Account ID or email address
        id: String,
    },
}

#[derive(Subcommand)]
enum FlagCmd {
    /// Add a flag to a message
    Add {
        /// Message UID
        uid: u32,
        /// Flag name (e.g. \\Seen, \\Flagged)
        flag: String,
        /// IMAP folder
        #[arg(long, default_value = "INBOX")]
        folder: String,
        /// Account ID or email
        #[arg(long)]
        account: Option<String>,
    },
    /// Remove a flag from a message
    Remove {
        /// Message UID
        uid: u32,
        /// Flag name
        flag: String,
        /// IMAP folder
        #[arg(long, default_value = "INBOX")]
        folder: String,
        /// Account ID or email
        #[arg(long)]
        account: Option<String>,
    },
}

#[derive(Subcommand)]
enum AttachmentCmd {
    /// List attachments for a message
    List {
        /// Message UID
        uid: u32,
        /// IMAP folder
        #[arg(long, default_value = "INBOX")]
        folder: String,
        /// Account ID or email
        #[arg(long)]
        account: Option<String>,
    },
    /// Download an attachment
    Download {
        /// Message UID
        uid: u32,
        /// Attachment filename
        filename: String,
        /// Output path
        #[arg(long)]
        output: Option<String>,
        /// IMAP folder
        #[arg(long, default_value = "INBOX")]
        folder: String,
        /// Account ID or email
        #[arg(long)]
        account: Option<String>,
    },
}

#[derive(Subcommand)]
enum DraftCmd {
    /// Create a new draft
    Create {
        /// Recipient
        #[arg(long)]
        to: String,
        /// Subject
        #[arg(long)]
        subject: Option<String>,
        /// Body text
        #[arg(long)]
        body: Option<String>,
        /// Account ID or email
        #[arg(long)]
        account: Option<String>,
    },
    /// List drafts
    List {
        /// Account ID or email
        #[arg(long)]
        account: Option<String>,
    },
    /// Send a draft
    Send {
        /// Draft ID
        id: String,
        /// Account ID or email
        #[arg(long)]
        account: Option<String>,
    },
    /// Discard a draft
    Discard {
        /// Draft ID
        id: String,
    },
}

#[derive(Subcommand)]
enum LicenseCmd {
    /// Activate a license key
    Activate {
        /// License key
        key: String,
    },
    /// Show current license status
    Status,
}

#[derive(Subcommand)]
enum ActionsCmd {
    /// Tail the action log
    Tail {
        /// Number of entries
        #[arg(long, default_value = "20")]
        limit: u32,
        /// Account ID or email
        #[arg(long)]
        account: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Accounts { subcommand } => commands::accounts::run(subcommand, cli.json),
        Commands::Inbox { .. } => {
            eprintln!("Not yet implemented: inbox (transport layer pending)");
            std::process::exit(1);
        }
        Commands::Read { .. } => {
            eprintln!("Not yet implemented: read (transport layer pending)");
            std::process::exit(1);
        }
        Commands::Search { .. } => {
            eprintln!("Not yet implemented: search (transport layer pending)");
            std::process::exit(1);
        }
        Commands::Send { .. } => {
            eprintln!("Not yet implemented: send (transport layer pending)");
            std::process::exit(1);
        }
        Commands::Move { .. } => {
            eprintln!("Not yet implemented: move (transport layer pending)");
            std::process::exit(1);
        }
        Commands::Copy { .. } => {
            eprintln!("Not yet implemented: copy (transport layer pending)");
            std::process::exit(1);
        }
        Commands::Delete { .. } => {
            eprintln!("Not yet implemented: delete (transport layer pending)");
            std::process::exit(1);
        }
        Commands::Flag { .. } => {
            eprintln!("Not yet implemented: flag (transport layer pending)");
            std::process::exit(1);
        }
        Commands::Folders { .. } => {
            eprintln!("Not yet implemented: folders (transport layer pending)");
            std::process::exit(1);
        }
        Commands::Attachment { .. } => {
            eprintln!("Not yet implemented: attachment (transport layer pending)");
            std::process::exit(1);
        }
        Commands::Draft { .. } => {
            eprintln!("Not yet implemented: draft (store integration pending)");
            std::process::exit(1);
        }
        Commands::Serve { port } => {
            eprintln!("Dashboard not yet implemented (port {port})");
            std::process::exit(1);
        }
        Commands::Compose { .. } => {
            eprintln!("License required — visit https://envelope-email.dev");
            std::process::exit(1);
        }
        Commands::License { subcommand } => match subcommand {
            LicenseCmd::Activate { key } => {
                eprintln!("Not yet implemented: license activate (key: {key})");
                std::process::exit(1);
            }
            LicenseCmd::Status => {
                println!("License: unlicensed (free tier)");
                Ok(())
            }
        },
        Commands::Attributes { .. } => {
            eprintln!("Not yet implemented: attributes");
            std::process::exit(1);
        }
        Commands::Actions { subcommand } => match subcommand {
            ActionsCmd::Tail { .. } => {
                eprintln!("Not yet implemented: actions tail");
                std::process::exit(1);
            }
        },
    };

    if let Err(e) = result {
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    }
}
