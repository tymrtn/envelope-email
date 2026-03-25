# CLAUDE.md — Envelope Email (Rust)

## What This Is

Envelope Email is a **clean email client** — BYO-mailbox, IMAP/SMTP, with agent-native primitives (JSON on every command, auto-discovery, scriptable). It gives OpenClaw agents email capabilities.

**Envelope is NOT a governance/scoring product.** The scoring layer, blind attribution, and send-zone routing belong to **Governor** — a separate product that hooks into Envelope externally. Never add governance language, scoring attributes, or `--attr` flags to this codebase. If someone asks for scoring features, the answer is "that's Governor."

## Repo & License

- **GitHub:** tymrtn/envelope-email-rs
- **License:** FSL-1.1-ALv2 (see LICENSE)
- **Copyright:** 2026 Tyler Martin

## Workspace Structure (4 crates)

```
crates/
├── cli/          # Binary crate — clap-based CLI (`envelope-email`)
│   └── src/
│       ├── main.rs           # CLI arg parsing + dispatch
│       └── commands/         # One file per command group
│           ├── accounts.rs   # accounts add/list/remove
│           ├── attachments.rs # attachment list/download
│           ├── drafts.rs     # draft create/list/send/discard
│           ├── flags.rs      # flag add/remove
│           ├── folders.rs    # list IMAP folders
│           ├── inbox.rs      # list messages
│           ├── messages.rs   # move/copy/delete
│           ├── read.rs       # read single message
│           ├── search.rs     # IMAP search
│           ├── send.rs       # send email via SMTP
│           ├── serve.rs      # start dashboard server
│           ├── common.rs     # shared helpers
│           └── mod.rs
├── email/        # Library — IMAP client, SMTP sender, DNS auto-discovery
│   └── src/
│       ├── discovery.rs      # MX/SRV → IMAP/SMTP host resolution
│       ├── imap.rs           # IMAP operations (async-imap + rustls)
│       ├── smtp.rs           # SMTP send (lettre + rustls)
│       ├── errors.rs
│       └── lib.rs
├── store/        # Library — SQLite persistence, crypto, models
│   └── src/
│       ├── accounts.rs       # Account CRUD
│       ├── action_log.rs     # Action audit log (for Governor integration)
│       ├── crypto.rs         # AES-GCM encryption, Argon2 key derivation
│       ├── db.rs             # Database init + connection
│       ├── drafts.rs         # Draft CRUD
│       ├── license_store.rs  # License key storage
│       ├── models.rs         # Shared data models
│       ├── errors.rs
│       └── lib.rs
└── dashboard/    # Library — Axum-based localhost web UI
    └── src/
        └── lib.rs            # REST API + embedded HTML dashboard
```

## Build & Test

```bash
# Build all crates
cargo build

# Build release binary
cargo build --release

# Run tests
cargo test

# Run the CLI directly
cargo run -p envelope-email-cli -- inbox --json

# Check formatting + lints
cargo fmt --check
cargo clippy
```

## CLI Commands (complete list from main.rs)

| Command | Status | Description |
|---------|--------|-------------|
| `accounts add/list/remove` | ✅ Implemented | Manage email accounts |
| `inbox` | ✅ Implemented | List messages in folder |
| `read <uid>` | ✅ Implemented | Read single message |
| `search "<query>"` | ✅ Implemented | IMAP search |
| `send` | ✅ Implemented | Send via SMTP |
| `move <uid>` | ✅ Implemented | Move message to folder |
| `copy <uid>` | ✅ Implemented | Copy message to folder |
| `delete <uid>` | ✅ Implemented | Delete message |
| `flag add/remove` | ✅ Implemented | Manage message flags |
| `folders` | ✅ Implemented | List IMAP folders |
| `attachment list/download` | ✅ Implemented | Manage attachments |
| `draft create/list/send/discard` | ✅ Implemented | Draft management |
| `serve` | ✅ Implemented | Localhost dashboard |
| `compose` | 🔒 Stub (license gate) | Licensed tier placeholder |
| `license activate/status` | ⚠️ Partial | Status works; activate is stub |
| `attributes` | ⚠️ Stub | Not yet implemented |
| `actions tail` | ⚠️ Stub | Not yet implemented |

## Governor Integration Points

Governor hooks into Envelope through these existing infrastructure pieces — **without Envelope knowing about governance:**

1. **Action Log** (`store/src/action_log.rs`) — Records agent actions with confidence scores and justification. Governor reads this externally.
2. **License Store** (`store/src/license_store.rs`) — License activation gates compose/attributes/actions commands. Governor manages the license.
3. **`compose` command** — Currently a license-gated stub. Governor will provide the compose-with-scoring flow.
4. **`attributes` command** — Stub that will list scoring attributes when Governor is active.
5. **`actions tail` command** — Stub that will show Governor's decisions.

The pattern: Envelope stores data and provides stubs. Governor fills them in. Envelope stays clean.

## Rules for Contributors

1. **No governance language in Envelope.** No "scoring," "blind attribution," "send zones," "governance," or `--attr` flags in user-facing code or docs. That's Governor.
2. **JSON on every command.** Every command must support `--json` for agent consumption.
3. **Auto-discovery by default.** Users provide email + password; IMAP/SMTP hosts are discovered via DNS.
4. **Credentials via pluggable backend.** Passwords encrypted with AES-256-GCM in SQLite. The master passphrase is managed by `--credential-store`:
   - `file` (default): encrypted in `~/.config/envelope-email/credentials.json`, keyed by `ENVELOPE_MASTER_KEY` env var or a machine-specific seed (hostname + username). Works on headless Linux, locked-screen macOS, servers — zero external deps.
   - `keychain`: OS keychain via `keyring` crate (requires `keychain` cargo feature). Use for interactive desktop workflows.
   The file backend is the default because it works everywhere. Keychain is opt-in.
5. **SQLite for state.** All persistent data in `~/.config/envelope-email/` via rusqlite.
6. **Every file starts with the copyright header:**
   ```rust
   // Copyright (c) 2026 Tyler Martin
   // Licensed under FSL-1.1-ALv2 (see LICENSE)
   ```
