# Envelope Email CLI

BYO mailbox email client. Replaces Himalaya for OpenClaw agents.

## Account Management

```bash
# Add account (auto-discovers SMTP/IMAP)
envelope-email accounts add --email <email> --password <app-password>

# List accounts
envelope-email accounts list [--json]

# Remove account
envelope-email accounts remove <id-or-email>
```

## Reading Email

```bash
# List inbox (most recent first)
envelope-email inbox [--folder INBOX] [--limit 50] [--json]

# Read a specific message by UID
envelope-email read <uid> [--folder INBOX] [--json]

# Search messages (IMAP search syntax)
envelope-email search "<query>" [--folder INBOX] [--limit 10] [--json]

# List folders
envelope-email folders [--json]
```

## Sending Email

```bash
# Direct send (no scoring)
envelope-email send --to <addr> --subject <sub> --body <body> \
  [--cc <addr>] [--bcc <addr>] [--reply-to <addr>] [--account <id>]
```

## Message Management

```bash
# Move message to folder
envelope-email move <uid> --to-folder <folder> [--folder INBOX]

# Copy message to folder
envelope-email copy <uid> --to-folder <folder> [--folder INBOX]

# Delete message
envelope-email delete <uid> [--folder INBOX]

# Add/remove flags
envelope-email flag add <uid> --flag <flag>
envelope-email flag remove <uid> --flag <flag>
# Flags: seen, flagged, answered, draft, deleted
```

## Attachments

```bash
# Download attachments from a message
envelope-email attachment download <uid> [--dir ~/Downloads] [--folder INBOX]
```

## Draft Management

```bash
# List drafts
envelope-email draft list [--status pending_review] [--json]

# Get draft status
envelope-email draft status <draft-id>

# Discard a draft
envelope-email draft reject <draft-id> --feedback "reason"
```

## Output Format

All commands support `--json` for structured output. Default is human-readable table format.

## Account Resolution

If `--account` is not specified, the default (first added) account is used. Specify by ID or email address.
