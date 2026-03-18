# Envelope Email CLI

BYO mailbox email client with agent-native primitives. Replaces Himalaya for OpenClaw agents.

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
envelope-email inbox [--folder INBOX] [--limit 50] [--account <id>] [--json]

# Read a specific message by UID
envelope-email read <uid> [--folder INBOX] [--account <id>] [--json]

# Search messages (IMAP search syntax)
envelope-email search "<query>" [--folder INBOX] [--limit 10] [--account <id>] [--json]

# List folders
envelope-email folders [--account <id>] [--json]
```

## Sending Email

```bash
# Direct send (no scoring — free tier)
envelope-email send --to <addr> --subject <sub> --body <body> \
  [--html <html>] [--cc <addr>] [--bcc <addr>] [--reply-to <addr>] [--account <id>] [--json]
```

## Compose with Attribution Scoring (licensed tier)

```bash
# Compose with blind attribution scoring
envelope-email compose --to <addr> --subject <sub> --body <body> \
  --attr <attribute> [--attr <attribute> ...] \
  [--html <html>] [--cc <addr>] [--bcc <addr>] [--account <id>] [--json]

# Reply with scoring (fetches original, adds In-Reply-To)
envelope-email reply <uid> --body <body> [--attr <attribute> ...] \
  [--folder INBOX] [--account <id>] [--json]

# Forward with scoring
envelope-email forward <uid> --to <addr> --body <body> [--attr <attribute> ...] \
  [--folder INBOX] [--account <id>] [--json]
```

Scoring routes messages to one of four zones:
- **sent** — auto-sent immediately via SMTP
- **delayed** — queued for 5-minute delay, then sent
- **pending_review** — saved to Drafts for human approval
- **blocked** — saved to Drafts, not sendable without review

## Message Management

```bash
# Move message to folder
envelope-email move <uid> --to-folder <folder> [--folder INBOX] [--account <id>]

# Copy message to folder
envelope-email copy <uid> --to-folder <folder> [--folder INBOX] [--account <id>]

# Delete message
envelope-email delete <uid> [--folder INBOX] [--account <id>]

# Add/remove flags
envelope-email flag add <uid> <flag> [--folder INBOX] [--account <id>]
envelope-email flag remove <uid> <flag> [--folder INBOX] [--account <id>]
# Flags: seen, flagged, answered, draft, deleted
```

## Draft Management

```bash
# Create a draft
envelope-email draft create --to <addr> [--subject <sub>] [--body <body>] [--account <id>] [--json]

# List drafts
envelope-email draft list [--account <id>] [--json]

# Send a draft via SMTP
envelope-email draft send <draft-id> [--account <id>] [--json]

# Discard a draft
envelope-email draft discard <draft-id> [--json]
```

## Attributes (licensed tier)

```bash
# List available scoring attributes (keys + descriptions, no weights)
envelope-email attributes [--json]
```

## Action Log (licensed tier)

```bash
# View recent governor decisions
envelope-email actions tail [--limit 20] [--account <id>] [--json]
```

## License Management

```bash
# Activate a license key
envelope-email license activate <key>

# Show license status
envelope-email license status
```

## Dashboard

```bash
# Start localhost account management UI
envelope-email serve [--port 3141]
```

## Output Format

All commands support `--json` for structured output. Default is human-readable table format.

## Account Resolution

If `--account` is not specified, the default (first added) account is used. Specify by ID or email address.

## Attribute Reference

| Key | Category | Description |
|-----|----------|-------------|
| reply_to_known | relationship | Replying to a known contact |
| reply_in_thread | relationship | Continuing an existing thread |
| known_contact | relationship | Recipient is in the contact book |
| frequent_contact | relationship | Recipient exchanged 5+ messages in 30 days |
| mutual_thread | relationship | Both parties have sent in this thread |
| recent_inbound | relationship | Received a message from recipient within 7 days |
| informational | intent | Message is purely informational |
| scheduling | intent | Message involves scheduling or calendar |
| acknowledgment | intent | Simple acknowledgment or confirmation |
| request_action | intent | Requesting the recipient take an action |
| commitment | intent | Making a promise or commitment |
| delegation | intent | Delegating a task or responsibility |
| escalation | intent | Escalating an issue or complaint |
| low_stakes | stakes | Error would be trivial to correct |
| medium_stakes | stakes | Error would require a follow-up to fix |
| high_stakes | stakes | Error could damage a relationship or reputation |
| financial | stakes | Message involves money, invoices, or payments |
| legal | stakes | Message has legal implications |
| irreversible | stakes | Action described cannot be undone |
| short_body | content | Message body is under 100 words |
| has_attachment | content | Message includes one or more attachments |
| has_link | content | Message body contains URLs |
| has_pii | content | Message contains personal identifiable info |
| template_match | content | Body matches a known safe template |
| agent_drafted | content | Message was drafted by an AI agent |
| human_edited | content | Message was edited by the human after draft |
| internal | domain | Recipient is on the same domain |
| trusted_domain | domain | Recipient domain is on the trusted list |
| unknown_domain | domain | Recipient domain has never been contacted |
| freemail | domain | Recipient is on a freemail provider |
| disposable_domain | domain | Recipient domain is a known disposable service |
| gov_domain | domain | Recipient is a government domain |
| single_recipient | recipient | Message has exactly one recipient |
| small_group | recipient | Message has 2-5 recipients |
| large_group | recipient | Message has 6+ recipients |
| has_bcc | recipient | Message uses BCC |
| first_contact | recipient | First message ever to this recipient |
