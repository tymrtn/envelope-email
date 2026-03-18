# Envelope Email

BYO mailbox email client with agent-native primitives. Turn any IMAP/SMTP account into a programmable email interface.

## Install

```bash
# Homebrew (macOS/Linux)
brew install tymrtn/tap/envelope-email

# Cargo (free tier — no scoring)
cargo install envelope-email

# From source
git clone https://github.com/tymrtn/envelope-email.git
cd envelope-email
cargo build --release
```

## Quick Start

```bash
# Add your email account (auto-discovers SMTP/IMAP settings)
envelope-email accounts add --email you@gmail.com --password <app-password>

# Check your inbox
envelope-email inbox

# Read a message
envelope-email read 42

# Send an email
envelope-email send --to someone@example.com --subject "Hello" --body "Hi there"

# Search messages
envelope-email search "FROM john@co.com"
```

## Commands

### Account Management

```bash
envelope-email accounts add --email <email> --password <password>
envelope-email accounts list [--json]
envelope-email accounts remove <id>
```

### Reading Email

```bash
envelope-email inbox [--folder INBOX] [--limit 50] [--json]
envelope-email read <uid> [--folder INBOX] [--json]
envelope-email search "<query>" [--folder INBOX] [--limit 10] [--json]
envelope-email folders [--json]
```

### Sending Email

```bash
envelope-email send --to <addr> --subject <sub> --body <body> [--cc <addr>] [--bcc <addr>]
```

### Message Management

```bash
envelope-email move <uid> --to-folder Archive [--folder INBOX]
envelope-email copy <uid> --to-folder Important [--folder INBOX]
envelope-email delete <uid> [--folder INBOX]
envelope-email flag add <uid> --flag seen
envelope-email flag remove <uid> --flag flagged
```

### Attachments

```bash
envelope-email attachment download <uid> [--dir ~/Downloads]
```

### Dashboard

```bash
envelope-email serve [--port 8080]
```

## JSON Output

Every command supports `--json` for agent/script consumption:

```bash
envelope-email inbox --json | jq '.[0].subject'
```

## License

FSL-1.1-ALv2 — see [LICENSE](LICENSE) for details.

Copyright 2026 Tyler Martin.
