# Personalized Junk Disposition — Feature Specification

**Date:** 2026-03-24
**Status:** Draft
**Author:** Tyler Martin (via Claude Code session)
**Priority:** Future release — currently prototyped at workspace level via OpenClaw cron

---

## Problem

Email junk filtering is binary (spam/not-spam) and sender-level. Real users have situational, personalized disposal patterns:

- A newsletter is junk during crunch week but wanted on weekends
- A sender is relevant to one project but noise for another account
- Marketing from a vendor is junk until you're actually shopping for that thing
- GitHub notifications matter for active repos but are noise for archived ones

Existing spam filters (SpamAssassin, Gmail, etc.) are global, opaque, and not user-trainable. Envelope should learn from the user's actual behavior and build a local, private, personalized disposition model.

---

## Core Concept

**Behavioral learning from implicit signals.** Every user action is a training signal:

| Action | Signal |
|--------|--------|
| Delete/trash immediately | Strong negative |
| Archive without reading | Weak negative |
| Read but don't reply | Neutral |
| Read and reply | Positive |
| Reply within minutes | Strong positive |
| Star/flag | Strong positive |
| Move to specific folder | Contextual (folder = intent) |
| Mark as spam | Permanent negative |

The system observes these signals over time and builds per-account disposition profiles.

---

## Architecture

### Local-first, no cloud

All learning happens on-device. No email content leaves the machine. The model is a local SQLite table (or sidecar file) per account.

### Feature extraction

From each email, extract lightweight features (never store email body):

```
sender_address      → "newsletter@dev.to"
sender_domain       → "dev.to"
subject_tokens      → ["weekly", "digest", "dev"]
time_of_day         → "morning" | "afternoon" | "evening" | "night"
day_of_week         → "monday" ... "sunday"
thread_depth        → 0 (new) | 1+ (reply chain)
has_attachments     → bool
account_id          → which mailbox received it
list_unsubscribe    → bool (presence of List-Unsubscribe header)
from_contact_book   → bool (sender in user's sent history)
reply_history       → count of past replies to this sender
```

### Disposition categories

Not binary. A spectrum:

| Category | Meaning | Auto-action |
|----------|---------|-------------|
| `always_want` | User always engages | Pin to top / highlight |
| `usually_want` | User usually reads | Normal inbox |
| `sometimes_want` | Situational | Show but deprioritize |
| `usually_skip` | User rarely engages | Collapse / dim |
| `always_skip` | User never engages | Auto-archive or auto-trash (user configurable) |
| `blocked` | Explicit user block | Auto-trash, never show |

### Learning algorithm

Simple frequency-weighted scoring. No neural net needed.

For each `(sender, account)` pair, maintain:

```rust
struct SenderProfile {
    sender: String,
    account_id: Uuid,
    total_received: u32,
    total_read: u32,
    total_replied: u32,
    total_trashed: u32,
    total_archived_unread: u32,
    total_starred: u32,
    avg_time_to_open_secs: Option<f64>,
    last_interaction: DateTime<Utc>,
    disposition: Disposition,  // computed from above
    manual_override: Option<Disposition>,  // user explicit choice wins
}
```

Disposition is computed:

```
score = (replied * 3 + starred * 2 + read * 1) - (trashed * 3 + archived_unread * 1)
score_normalized = score / total_received

always_want:      score_normalized > 0.7
usually_want:     score_normalized > 0.3
sometimes_want:   score_normalized > -0.3
usually_skip:     score_normalized > -0.7
always_skip:      score_normalized <= -0.7
```

Manual overrides (`envelope junk allow <sender>`, `envelope junk block <sender>`) always win.

### Domain-level rollups

If 3+ senders from the same domain are `always_skip`, promote the entire domain to `usually_skip` (unless any sender from that domain is `usually_want` or better).

### Situational awareness (v2)

Later iteration: weight by context signals:

- **Time decay:** Recent actions weighted more than old ones (exponential decay, half-life 30 days)
- **Account context:** A sender might be wanted on `tyler@aposema.com` but junk on `hola@klasificados.net`
- **Project phase:** If HEARTBEAT.md (or a local config) says "focus mode: patent filing," deprioritize everything except legal/patent senders
- **Thread awareness:** If user replied to a thread, all future messages in that thread are `always_want` regardless of sender score

---

## CLI Interface

```bash
# View current junk profile
envelope junk status
envelope junk status --account tmartin@aposema.com

# Manual overrides
envelope junk allow <sender|domain>        # never auto-trash
envelope junk block <sender|domain>        # always auto-trash
envelope junk reset <sender|domain>        # remove override, return to learned behavior

# Bulk review (interactive)
envelope junk review                       # show usually_skip senders, confirm/deny

# Import from current trash (bootstrap)
envelope junk learn                        # scan trash folders, build initial profiles

# Export (for backup/debugging)
envelope junk export --format json > junk-profile.json
```

---

## Storage

New SQLite table in the Envelope database:

```sql
CREATE TABLE sender_profiles (
    id INTEGER PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id),
    sender_address TEXT NOT NULL,
    sender_domain TEXT NOT NULL,
    total_received INTEGER DEFAULT 0,
    total_read INTEGER DEFAULT 0,
    total_replied INTEGER DEFAULT 0,
    total_trashed INTEGER DEFAULT 0,
    total_archived_unread INTEGER DEFAULT 0,
    total_starred INTEGER DEFAULT 0,
    avg_time_to_open_secs REAL,
    last_interaction TEXT,  -- ISO 8601
    disposition TEXT NOT NULL DEFAULT 'sometimes_want',
    manual_override TEXT,  -- NULL = learned, non-NULL = user explicit
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(account_id, sender_address)
);

CREATE INDEX idx_sender_profiles_disposition ON sender_profiles(disposition);
CREATE INDEX idx_sender_profiles_domain ON sender_profiles(sender_domain);
```

---

## Event hooks

Envelope already processes email actions. Add disposition tracking to existing action handlers:

```rust
// On any email action, update sender profile
fn on_email_action(action: EmailAction, email: &Email) {
    let profile = get_or_create_sender_profile(email.account_id, &email.from);
    match action {
        EmailAction::Read => profile.total_read += 1,
        EmailAction::Reply => profile.total_replied += 1,
        EmailAction::Trash => profile.total_trashed += 1,
        EmailAction::Archive => profile.total_archived_unread += 1,
        EmailAction::Star => profile.total_starred += 1,
        _ => {}
    }
    profile.recompute_disposition();
    profile.save();
}
```

---

## Inbox integration

When listing inbox, annotate each email with its sender's disposition:

```json
{
  "id": "...",
  "from": "newsletter@dev.to",
  "subject": "Weekly Digest",
  "disposition": "usually_skip",
  "disposition_source": "learned"
}
```

CLI can filter:

```bash
envelope inbox --skip-junk              # hide usually_skip and always_skip
envelope inbox --only-important          # show only always_want and usually_want
envelope inbox                           # show everything, annotated
```

API consumers (MCP server, OpenClaw triage cron) can use disposition to prioritize without reimplementing the logic.

---

## Privacy guarantees

- No email body content is stored in profiles — only sender address, domain, and action counts
- No data leaves the device — all computation is local SQLite
- No third-party model or API involved
- User can `envelope junk export` to audit exactly what's stored
- User can `envelope junk reset --all` to wipe all learned profiles

---

## Current Prototype

A workspace-level prototype exists at `~/.openclaw/workspace-dev/junk-profile.json`, maintained by an OpenClaw cron job (`junk-profile-learner`, daily at 4am). It scans trash vs inbox across 12 Envelope accounts and builds sender/domain pattern lists. The triage cron (`tmartin-inbox-monitor`, hourly) references this file to auto-skip known junk.

This spec describes the production version built into Envelope itself, replacing the external JSON file with native SQLite storage and the cron job with real-time event hooks.

---

## Competitive positioning

No CLI email client does this. Himalaya, aerc, mutt, neomutt — none learn from user behavior. This is a genuine differentiator for Envelope: a privacy-first, local-only, behavioral email classifier that gets smarter the more you use it. No cloud. No subscription. No training data shared with anyone.

For the Show HN: "Envelope learns what you throw away."
