# Threading System Security & Sensibility Review

**Reviewer:** The Enforcer (GPT-5.4 / Codex)
**Date:** 2026-03-28
**Scope:** Envelope Email RS threading system — all new files
**Verdict:** No critical vulnerabilities. One HIGH bug (UTF-8 panic), several MEDIUM design issues. Solid foundation.

---

## Security Findings

### S1. SQL Injection — CLEAN ✅
**Severity:** INFO
**Files:** `crates/store/src/threads.rs`

All SQL queries use rusqlite's `params![]` macro with positional placeholders (`?1`, `?2`, etc.). No string interpolation of user or IMAP-sourced data into SQL.

The `find_thread_by_references()` method dynamically builds an `IN (...)` clause, but correctly generates numbered placeholders (`?1`, `?2`, ..., `?N`) and passes actual values through the parameter binding API. This is safe.

The `THREAD_COLS` and `THREAD_MSG_COLS` constants used in `format!()` are compile-time string literals containing only column names. No injection vector.

**Status:** No action needed.

---

### S2. Cross-Account Data Leakage
**Severity:** MEDIUM
**Files:** `crates/store/src/threads.rs`, `crates/email/src/threading.rs`

Several thread lookup functions lack `account_id` scoping:

| Function | account_id in WHERE? |
|---|---|
| `get_thread()` | ❌ |
| `find_thread_by_subject()` | ✅ |
| `list_threads()` | ✅ (optional) |
| `find_thread_by_message_id()` | ❌ |
| `find_thread_by_references()` | ❌ |
| `find_thread_by_uid()` | ❌ |
| `get_thread_messages()` | ❌ |
| `get_thread_context_for_uid()` | ❌ |

**Impact:** In a multi-account setup (e.g., `tyler@aposema.com` + `skippy@aposema.com`), the threading algorithm could link a message arriving in account A to a thread belonging to account B if they share Message-IDs in the References chain. This is a design choice more than a bug — cross-account threading can be desirable — but it should be explicit, not accidental.

The `list_threads(None, ...)` path returns threads from ALL accounts without warning.

**Risk context:** Envelope is a local CLI tool (single user, multi-account). This is not a server-side multi-tenant issue. Severity is MEDIUM because it could confuse users and cause incorrect thread grouping, not because it leaks data to unauthorized parties.

**Recommendation:**
1. Add `account_id` parameter to `find_thread_by_message_id()` and `find_thread_by_references()`.
2. Or: explicitly document cross-account threading as a feature and add a `--cross-account` flag.
3. `list_threads()` with `account_id=None` should be intentional (admin view), not the default.

---

### S3. Input Validation — Missing Length Limits
**Severity:** LOW
**Files:** `crates/email/src/threading.rs`, `crates/store/src/threads.rs`

Headers extracted from parsed emails (Message-ID, In-Reply-To, References, subject) are stored directly in SQLite without length validation. A maliciously crafted email could have:

- A 1MB Message-ID header
- A References header with 10,000 entries
- A 100KB subject line

These would be stored verbatim. SQLite handles large TEXT columns fine, but:
- The `find_thread_by_references()` would generate a query with 10,000 placeholders
- The `normalize_subject()` would allocate and process a 100KB string on every lookup

**No format validation** is performed on Message-IDs (RFC 2822 specifies `<localpart@domain>` format). Non-conforming values are accepted silently.

**Recommendation:**
1. Truncate `message_id` to 998 chars (RFC 2822 max line length).
2. Limit References to first 50 entries.
3. Truncate subject to 1000 chars before normalization.
4. Optionally validate Message-ID format (angle brackets + @ sign).

---

### S4. Snippet Extraction — UTF-8 Panic Bug 🔴
**Severity:** HIGH
**File:** `crates/email/src/threading.rs`, line ~85

```rust
pub fn extract_snippet(text: &str, max_len: usize) -> String {
    // ...
    if collapsed.len() <= max_len {
        collapsed
    } else {
        format!("{}...", &collapsed[..max_len])  // ← PANIC HERE
    }
}
```

`collapsed` is a `String` (UTF-8). `collapsed.len()` returns **byte** length. `&collapsed[..max_len]` slices by **byte** index. If byte 200 falls inside a multi-byte UTF-8 character, Rust panics with:

```
thread 'main' panicked at 'byte index 200 is not a char boundary'
```

This WILL crash the thread builder when processing any email containing multi-byte characters (accented letters like é, ñ, ü; CJK characters; emoji; Cyrillic; Arabic) near the 200-byte boundary. Given that Tyler operates in Spanish and communicates in multiple languages, this is near-certain to trigger.

**Fix:**

```rust
if collapsed.len() <= max_len {
    collapsed
} else {
    // Find a valid char boundary at or before max_len
    let mut end = max_len;
    while end > 0 && !collapsed.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &collapsed[..end])
}
```

Or use the `floor_char_boundary` method (nightly) or a crate like `unicode-segmentation`.

**Status:** Must fix before production use.

---

### S5. IMAP Injection
**Severity:** LOW
**Files:** `crates/email/src/threading.rs`

The `scan_folder_for_threads()` function passes the `folder` parameter to `client.session_mut().select(folder)`. The `async-imap` library handles mailbox name quoting per the IMAP protocol, so direct IMAP injection is not possible through the library's API.

The `fetch_query` strings are constructed from `u32` arithmetic (`format!("{start}:*")`), so they cannot contain injected content.

The sent folder name comes from either:
1. Hardcoded `SENT_FOLDER_CANDIDATES` constants
2. The IMAP server's folder list response (via `list_folders`)
3. Cached database values

None of these are directly user-controlled (except the CLI `--folder` argument, which goes through `async-imap`'s quoting).

**Status:** No action needed.

---

### S6. Snippet Memory Amplification
**Severity:** LOW
**File:** `crates/email/src/threading.rs`

`extract_snippet()` creates two intermediate full-size allocations before truncating:

```rust
let cleaned: String = text.lines()
    .filter(|line| !line.trim_start().starts_with('>'))
    .collect::<Vec<_>>()
    .join(" ");  // Full allocation #1

let collapsed: String = cleaned.split_whitespace()
    .collect::<Vec<_>>()
    .join(" ");  // Full allocation #2
```

For a 10MB email body, this allocates ~20MB before truncating to 200 chars. Not exploitable for RCE, but a crafted email could cause memory pressure during batch processing.

**Recommendation:** Use iterators with early termination:
```rust
// Take only the first ~max_len*2 characters before processing
let prefix: String = text.chars().take(max_len * 4).collect();
// Then filter and truncate from the prefix
```

---

## Sensibility Findings

### D1. Threading Algorithm — Solid with Known Limitations
**Severity:** INFO
**File:** `crates/email/src/threading.rs`

The threading priority chain is:
1. In-Reply-To → find existing thread
2. References → find existing thread
3. Own Message-ID → find existing thread (handles out-of-order delivery)
4. Subject normalization + address overlap → fuzzy match

**Edge cases handled:**
- ✅ Missing Message-ID (falls through to subject matching)
- ✅ Missing In-Reply-To (tries References next)
- ✅ Multiple In-Reply-To values (takes first: `split_whitespace().next()`)
- ✅ Empty References (returns early with `None`)
- ✅ No threading headers at all (subject fallback with address overlap guard)

**Edge cases NOT handled:**
- ❌ **Circular references** — Not a runtime issue (lookups are flat, not graph walks), but could cause incorrect grouping if message A references B and B references A with different subjects.
- ❌ **Thread merging** — If a reply references messages from two different existing threads, only the first match is used. The threads are never merged. This is a known limitation of simple threading algorithms.
- ❌ **Messages without subjects** — `normalize_subject("")` returns `""`, and the code checks `if normalized.is_empty() { return None; }`, which correctly skips subject-based matching. But all no-subject emails become singleton threads. Acceptable.

**Status:** The algorithm is correct for the common case. JWZ threading (the full algorithm from Jamie Zawinski) handles thread merging, but the complexity isn't justified for a CLI tool at this stage.

---

### D2. Subject Normalization — Missing Localized Prefixes
**Severity:** MEDIUM
**File:** `crates/email/src/threading.rs`

Currently strips: `Re:`, `Fwd:`, `Fw:`, `Re[N]:`

Missing localized variants used by major email clients:

| Prefix | Language | Client |
|---|---|---|
| `Sv:` | Swedish/Danish/Norwegian | Outlook Nordic |
| `Aw:` / `AW:` | German ("Antwort") | Outlook DE |
| `Wg:` / `WG:` | German ("Weitergeleitet") | Outlook DE |
| `Vs:` | Finnish | Outlook FI |
| `Ref:` | Various | Some mobile clients |
| `Rif:` | Italian | Outlook IT |
| `Doorst:` | Dutch | Outlook NL |
| `Tr:` | French/Turkish | Outlook FR/TR |
| `Rv:` | Vietnamese | Outlook VN |

**Impact:** Emails from non-English Outlook users will create new threads instead of grouping with their parent. Tyler operates in Spain and communicates in Spanish, German, French, and Russian, so this will cause fragmented threads with European correspondents.

**Recommendation:** Add all common prefixes. The STRSTRP RFC draft lists the full set. At minimum add: `Sv:`, `Aw:`, `AW:`, `Wg:`, `WG:`, `Tr:`, `Rif:`, `Ref:`.

---

### D3. Incremental Sync — Missing UIDVALIDITY Check 🔴
**Severity:** HIGH
**File:** `crates/email/src/threading.rs`, `crates/store/src/threads.rs`

The incremental sync tracks `last_uid` per folder/account but does NOT track or verify `UIDVALIDITY`.

Per RFC 3501 §2.3.1.1:
> If unique identifiers from an earlier session fail to persist in this session, the unique identifier validity value MUST be greater than the one used in the earlier session.

When UIDVALIDITY changes (caused by folder recreation, IMAP server migration, some backup restores), ALL previously stored UIDs become meaningless. The current code would:

1. Read `last_uid = 600` from sync state
2. Fetch UIDs `601:*` from the server
3. The server's new UID space might start at 1, so `601:*` returns nothing
4. Sync appears complete but missed ALL messages
5. Or worse: UIDs 1-600 in the new validity epoch are DIFFERENT messages than UIDs 1-600 in the old epoch, causing data corruption

**Recommendation:**
1. Store `uidvalidity` alongside `last_uid` in `thread_sync_state`.
2. On each sync, compare stored UIDVALIDITY with the SELECT response's UIDVALIDITY.
3. If changed: reset `last_uid` to 0 and do a full rescan for that folder.

```sql
ALTER TABLE thread_sync_state ADD COLUMN uidvalidity INTEGER;
```

```rust
let mailbox = client.session_mut().select(folder).await?;
let current_validity = mailbox.uid_validity.unwrap_or(0);
let stored_validity = db.get_uidvalidity(account_id, folder)?;

if Some(current_validity) != stored_validity {
    warn!("UIDVALIDITY changed for {folder}, resetting sync state");
    db.reset_sync_state(account_id, folder)?;
    // Full rescan
}
```

**Status:** Must fix. This is a correctness issue, not just theoretical.

---

### D4. Sent Folder Detection — Reasonable with Minor Issue
**Severity:** LOW
**File:** `crates/email/src/threading.rs`

The detection cascade is:
1. Check cached DB value → use it
2. Match against 7 hardcoded sent folder names → cache and use
3. Case-insensitive fuzzy: `lower.contains("sent") && !lower.contains("unsent")` → cache and use
4. No match → return None, skip sent scanning

**Minor issue:** The fuzzy match would incorrectly match folders like "Consent Forms", "Unresented Invoices", or any user folder with "sent" in the name. The `!lower.contains("unsent")` guard only catches one false positive pattern.

**Impact:** LOW. If a wrong folder is detected, it gets cached and all its messages get scanned for threading. The messages won't match any INBOX threads (different subjects/participants), so they'll just create orphan threads. Subsequent `detect_sent_folder` calls will use the cached wrong answer until the cache is cleared.

**When no sent folder is found:** The code gracefully skips sent folder scanning. Threads will be INBOX-only, missing outbound message context. Thread `has_reply` will always be false. Functional but incomplete.

**Recommendation:** Tighten the fuzzy match — require the folder name to start with "sent" or be in a known prefix path (e.g., `INBOX.Sent*`).

---

### D5. Performance — Per-Message `refresh_thread_stats` is Wasteful
**Severity:** MEDIUM
**File:** `crates/email/src/threading.rs`

Inside `process_fetched_messages`, for EVERY message processed:

```rust
if let Err(e) = db.refresh_thread_stats(&tid) { ... }
```

`refresh_thread_stats` runs three subqueries:
```sql
SELECT COUNT(*) FROM thread_messages WHERE thread_id = ?1
SELECT MIN(date) FROM thread_messages WHERE thread_id = ?1
SELECT MAX(date) FROM thread_messages WHERE thread_id = ?1
```

For 500 messages across 2 folders (1000 total), that's 3000 extra queries. Many of these hit the same thread_id repeatedly (e.g., 10 messages in one thread = 10 redundant stat refreshes for the same thread, where only the last one matters).

**Recommendation:** Collect modified thread_ids in a `HashSet`, then refresh stats once per thread at the end of the batch:

```rust
let mut dirty_threads: HashSet<String> = HashSet::new();
// ... inside the loop:
dirty_threads.insert(tid.clone());
// ... after the loop:
for tid in &dirty_threads {
    db.refresh_thread_stats(tid)?;
}
```

This reduces 3000 queries to ~3 * (number of unique threads), typically 50-150 queries.

**Additional concern:** The subject-fallback path calls `get_thread_messages()` which loads ALL messages in a candidate thread to check address overlap. For a thread with 500 messages, this is a large allocation just to check if any address overlaps. A dedicated SQL query would be far more efficient:

```sql
SELECT 1 FROM thread_messages
WHERE thread_id = ?1
  AND (LOWER(from_address) IN (?, ?) OR LOWER(to_addresses) LIKE ?)
LIMIT 1
```

---

### D6. `filter_map(|r| r.ok())` Silently Swallows Errors
**Severity:** LOW
**File:** `crates/store/src/threads.rs`

Multiple query result collections use:
```rust
let rows = stmt.query_map(...)?;
rows.filter_map(|r| r.ok()).collect()
```

This silently drops any row that fails to deserialize. If a database migration left stale data or a column type changed, rows would vanish without warning.

**Recommendation:** Log a warning on `Err` before dropping:
```rust
rows.filter_map(|r| match r {
    Ok(v) => Some(v),
    Err(e) => { warn!("failed to map row: {e}"); None }
}).collect()
```

---

### D7. Thread Table Lacks Foreign Key to Accounts
**Severity:** INFO
**File:** `crates/store/src/db.rs`

```sql
CREATE TABLE IF NOT EXISTS threads (
    ...
    account_id TEXT NOT NULL
    -- No REFERENCES accounts(id)
);
```

The `thread_messages` table has `REFERENCES threads(thread_id)`, but `threads.account_id` has no foreign key constraint to `accounts.id`. If an account is deleted, orphan threads persist. Similarly, `thread_sync_state` and `detected_folders` lack FK constraints.

**Impact:** Orphan data accumulation over time. No data corruption risk since SQLite doesn't enforce FKs by default anyway (requires `PRAGMA foreign_keys = ON`).

**Recommendation:** Add FK constraints and add a cleanup function to purge threads/sync state for deleted accounts.

---

### D8. UID Integer Overflow
**Severity:** LOW
**File:** `crates/store/src/threads.rs`

UIDs are stored as `i64` in SQLite but handled as `u32` in Rust:
```rust
let uid_i64: i64 = row.get(2)?;
// ...
uid: uid_i64 as u32,
```

IMAP UIDs are 32-bit unsigned integers (max 4,294,967,295). The `as u32` cast from `i64` is safe for valid IMAP UIDs. However, if the database somehow contains a value > u32::MAX or negative, the cast would silently wrap. The storage as `i64` is correct (SQLite has no unsigned type), but the retrieval should validate the range.

**Status:** Theoretical. IMAP servers won't produce out-of-range UIDs.

---

## Summary

| # | Finding | Severity | Status |
|---|---|---|---|
| S1 | SQL injection | INFO | Clean ✅ |
| S2 | Cross-account leakage | MEDIUM | Design decision needed |
| S3 | Missing input length limits | LOW | Harden when convenient |
| S4 | **UTF-8 panic in snippet extraction** | **HIGH** | **Must fix** |
| S5 | IMAP injection | LOW | Clean ✅ |
| S6 | Snippet memory amplification | LOW | Optimize when convenient |
| D1 | Threading algorithm | INFO | Solid, known limitations |
| D2 | Missing localized subject prefixes | MEDIUM | Add before international use |
| D3 | **Missing UIDVALIDITY tracking** | **HIGH** | **Must fix** |
| D4 | Sent folder fuzzy match | LOW | Tighten when convenient |
| D5 | Per-message refresh_thread_stats | MEDIUM | Batch for performance |
| D6 | Silent error swallowing | LOW | Add logging |
| D7 | Missing FK constraints | INFO | Cosmetic |
| D8 | UID integer cast | LOW | Theoretical |

### Two Must-Fix Items

1. **S4 — UTF-8 panic:** `extract_snippet` will crash on multi-byte characters at the truncation boundary. Trivial fix, high impact.

2. **D3 — UIDVALIDITY:** Without tracking UIDVALIDITY, any IMAP folder recreation silently corrupts the sync state. Moderate fix, high correctness impact.

### Overall Assessment

The threading system is well-structured. SQL is properly parameterized, the threading algorithm handles common edge cases correctly, the IMAP interaction is safe, and the code is cleanly organized. The two HIGH issues are both fixable in under an hour. The MEDIUM items (cross-account scoping, localized prefixes, batch stats refresh) should be addressed before the system handles significant volume or international email.

No critical vulnerabilities found. Ship after fixing S4 and D3.
