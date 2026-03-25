# Audit Results — Envelope Email (Rust)

**Date:** 2026-03-23  
**Auditor:** Skippy (subagent: envelope-rs-audit)

---

## 1. README Accuracy

### Issues Found & Fixed

| # | Issue | Severity | Status |
|---|-------|----------|--------|
| 1 | `cargo install` line said "free tier — no scoring" — governance language from old Python version | 🔴 High | ✅ Fixed |
| 2 | `flag` command documented as `--flag seen` but code uses positional arg: `flag add <uid> <flag>` | 🟡 Medium | ✅ Fixed |
| 3 | `attachment download` documented as `<uid> [--dir ~/Downloads]` but code requires positional `<filename>` and uses `--output` not `--dir` | 🟡 Medium | ✅ Fixed |
| 4 | `attachment list` subcommand exists in code but was not documented in README | 🟡 Medium | ✅ Fixed |
| 5 | Draft management (`draft create/list/send/discard`) exists in code but not in README | 🟡 Medium | ✅ Fixed |
| 6 | `serve` default port shown as 8080 in README but code defaults to 3141 | 🟡 Medium | ✅ Fixed |
| 7 | README `inbox` shows `--limit 50` as example; code defaults to 25. Not wrong (it's an example), but worth noting | 🟢 Low | Noted |

### Governance Language Scan

- **README.md:** One instance found ("no scoring" in cargo install comment) — **fixed**.
- **Source code (.rs files):** Clean. No references to scoring, blind attribution, governance, or `--attr`.
- **Cargo.toml:** Clean.

### Positioning

README correctly positions as a BYO-mailbox email client. No over-promising. Clean and professional.

---

## 2. SKILL.md Accuracy

### 🔴 Critical: SKILL.md contains extensive old-Python governance content

The SKILL.md file has major contamination from the old Python version:

| Line | Content | Problem |
|------|---------|---------|
| 37 | "no scoring — free tier" | Governance language |
| 42-58 | "Compose with Attribution Scoring (licensed tier)" section | Entire section documents `--attr` flags, `reply`, `forward` commands that DON'T EXIST in Rust code |
| 59-62 | Scoring zones (sent/delayed/pending_review/blocked) | Governor concept, not Envelope |
| 102-103 | "List available scoring attributes" | Governor feature |
| 109 | "View recent governor decisions" | Literally says "governor" |
| 114-157 | Full attribute reference table (35 attributes) | Governor's domain |

**Recommendation:** SKILL.md needs a complete rewrite to match the Rust codebase. The `compose`, `reply`, and `forward` commands documented there do not exist. The attribute table belongs in Governor's docs.

---

## 3. Code ↔ README Command Matrix

### Commands in Code (main.rs) vs README

| Command | In Code | In README | Match |
|---------|---------|-----------|-------|
| `accounts add` | ✅ | ✅ | ✅ |
| `accounts list` | ✅ | ✅ | ✅ |
| `accounts remove` | ✅ | ✅ | ✅ |
| `inbox` | ✅ | ✅ | ✅ |
| `read` | ✅ | ✅ | ✅ |
| `search` | ✅ | ✅ | ✅ |
| `send` | ✅ | ✅ | ✅ |
| `move` | ✅ | ✅ | ✅ |
| `copy` | ✅ | ✅ | ✅ |
| `delete` | ✅ | ✅ | ✅ |
| `flag add/remove` | ✅ | ✅ | ✅ (fixed syntax) |
| `folders` | ✅ | ✅ | ✅ |
| `attachment list` | ✅ | ✅ | ✅ (added) |
| `attachment download` | ✅ | ✅ | ✅ (fixed args) |
| `draft create/list/send/discard` | ✅ | ✅ | ✅ (added) |
| `serve` | ✅ | ✅ | ✅ (fixed port) |
| `compose` | ✅ (stub) | ❌ | Intentional — license-gated stub |
| `license activate/status` | ✅ (partial) | ❌ | Not documented — fine for now |
| `attributes` | ✅ (stub) | ❌ | Not documented — Governor's domain |
| `actions tail` | ✅ (stub) | ❌ | Not documented — Governor's domain |

### Commands in SKILL.md That Don't Exist in Code

| Command | In SKILL.md | In Code | Verdict |
|---------|-------------|---------|---------|
| `compose --attr` | ✅ | ❌ | Old Python version — remove from SKILL.md |
| `reply` | ✅ | ❌ | Old Python version — remove from SKILL.md |
| `forward` | ✅ | ❌ | Old Python version — remove from SKILL.md |
| `attributes` (with scoring data) | ✅ | Stub only | Governor's domain — remove from SKILL.md |

---

## 4. CLAUDE.md

✅ Created at repo root with:
- What Envelope is (clean email client, no governance)
- Complete crate structure with file-level detail
- Build/test commands
- CLI command table with implementation status
- Governor integration points (how it hooks in externally)
- Contributor rules (no governance language, JSON on every command, etc.)
- License info

---

## 5. Summary

| Area | Grade | Notes |
|------|-------|-------|
| README accuracy | **B+** | Was mostly correct; 7 issues found and fixed |
| Governance contamination | **C** | README had 1 instance (fixed). SKILL.md is heavily contaminated (needs rewrite) |
| Code-docs alignment | **A-** | After fixes, all implemented commands are documented. Stubs intentionally omitted. |
| Positioning | **A** | Clean, professional email client. No overreach. |
| CLAUDE.md | **A** | Created from scratch with full crate-level detail |

### Remaining Action Items

1. **🔴 Rewrite SKILL.md** — Remove all scoring/governance/attribution content. Document only commands that exist in the Rust codebase. This is the biggest remaining issue.
2. **🟡 Consider documenting `license status`** — It works and is useful for users to check their tier.
3. **🟢 README `--account` flag** — Many commands support `--account <id>` but the README examples don't show it. Consider adding to at least one example.
