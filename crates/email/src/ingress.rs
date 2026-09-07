// Copyright (c) 2026 Tyler Martin
// Licensed under FSL-1.1-ALv2 (see LICENSE)

//! Bounds and normalization for untrusted inbound message content.
//!
//! These limits are availability controls, not malware scanning or a claim that
//! an accepted attachment is safe to open. Envelope never executes attachments.

use std::io::{self, Read};
use std::time::{Duration, Instant};

/// Largest RFC822 message fetched into memory from IMAP.
pub const MAX_RFC822_MESSAGE_BYTES: u32 = 25 * 1024 * 1024;
/// Largest decoded attachment copied from a parsed message.
pub const MAX_ATTACHMENT_BYTES: usize = 20 * 1024 * 1024;
/// Largest aggregate raw-message payload accepted by one evidence operation.
pub const MAX_EVIDENCE_TOTAL_BYTES: u64 = 250 * 1024 * 1024;
/// ZIP/DOCX container input cap for optional text extraction.
pub const MAX_DOCX_COMPRESSED_BYTES: usize = 16 * 1024 * 1024;
/// Maximum ZIP entries inspected by DOCX extraction.
pub const MAX_DOCX_ENTRIES: usize = 256;
/// Maximum expanded bytes in an inspected ZIP entry.
pub const MAX_DOCX_EXPANDED_BYTES: u64 = 32 * 1024 * 1024;
/// Maximum expanded/compressed ratio accepted by DOCX extraction.
pub const MAX_DOCX_EXPANSION_RATIO: u64 = 100;
/// CPU wall-clock budget for optional DOCX extraction.
pub const MAX_DOCX_EXTRACTION_TIME: Duration = Duration::from_secs(2);

/// Refuse optional DOCX text extraction once its complete CPU/wall-clock budget
/// is exhausted. This is deliberately separate so archive metadata inspection,
/// decompression, and XML-to-text conversion all use the same deadline.
pub fn ensure_docx_time(started: Instant) -> Result<(), String> {
    if started.elapsed() > MAX_DOCX_EXTRACTION_TIME {
        Err("docx_time_limit_exceeded".to_string())
    } else {
        Ok(())
    }
}

/// Reject a declared raw-message size before any MIME parser or byte copy sees it.
pub fn validate_rfc822_size(size: u32) -> Result<(), String> {
    if size > MAX_RFC822_MESSAGE_BYTES {
        return Err(format!(
            "declared RFC822.SIZE {size} exceeds {MAX_RFC822_MESSAGE_BYTES} bytes"
        ));
    }
    Ok(())
}

/// Reject a decoded attachment size before a caller copies its bytes.
pub fn validate_attachment_size(size: usize) -> Result<(), String> {
    if size > MAX_ATTACHMENT_BYTES {
        return Err(format!(
            "decoded attachment {size} bytes exceeds {MAX_ATTACHMENT_BYTES} bytes"
        ));
    }
    Ok(())
}

/// Make an attachment filename safe for an implicit local write. The original
/// filename remains provenance/display metadata and must not be used as a path.
pub fn normalize_attachment_filename(original: &str) -> String {
    let last_segment = original
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(original)
        .trim();

    let mut out = String::with_capacity(last_segment.len());
    for ch in last_segment.chars() {
        if ch.is_control() {
            continue;
        }
        if ch.is_alphanumeric() || matches!(ch, '.' | '-' | '_' | ' ' | '(' | ')') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    let out = out.replace("..", "_");
    let trimmed = out.trim().trim_start_matches('.').trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        "attachment.bin".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Normalize an attacker-supplied MIME value before it is displayed or used as
/// an HTTP response type. Invalid values deliberately become octet-stream.
pub fn normalize_content_type(value: &str) -> String {
    let media = value.split(';').next().unwrap_or_default().trim();
    let Some((major, minor)) = media.split_once('/') else {
        return "application/octet-stream".to_string();
    };
    if major.is_empty()
        || minor.is_empty()
        || minor.contains('/')
        || !major.bytes().chain(minor.bytes()).all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
                )
        })
    {
        return "application/octet-stream".to_string();
    }
    format!(
        "{}/{}",
        major.to_ascii_lowercase(),
        minor.to_ascii_lowercase()
    )
}

/// Validate metadata exposed by a ZIP archive before any entry is decompressed.
pub fn validate_docx_archive_limits<R: Read + io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    input_len: usize,
    started: Instant,
) -> Result<(), String> {
    ensure_docx_time(started)?;
    if input_len > MAX_DOCX_COMPRESSED_BYTES {
        return Err(format!(
            "docx_input_limit_exceeded: {input_len} bytes exceeds {MAX_DOCX_COMPRESSED_BYTES}"
        ));
    }
    if archive.len() > MAX_DOCX_ENTRIES {
        return Err(format!(
            "docx_entry_limit_exceeded: {} entries exceeds {MAX_DOCX_ENTRIES}",
            archive.len()
        ));
    }
    for index in 0..archive.len() {
        ensure_docx_time(started)?;
        let file = archive
            .by_index_raw(index)
            .map_err(|e| format!("zip metadata: {e}"))?;
        let expanded = file.size();
        let compressed = file.compressed_size();
        if expanded > MAX_DOCX_EXPANDED_BYTES {
            return Err(format!(
                "docx_expanded_limit_exceeded: entry {} has {expanded} bytes (limit {MAX_DOCX_EXPANDED_BYTES})",
                file.name()
            ));
        }
        if compressed == 0 {
            if expanded > 0 {
                return Err(format!(
                    "docx_ratio_limit_exceeded: entry {} has zero compressed size",
                    file.name()
                ));
            }
        } else if docx_expansion_ratio_exceeded(expanded, compressed) {
            return Err(format!(
                "docx_ratio_limit_exceeded: entry {} ratio exceeds {MAX_DOCX_EXPANSION_RATIO}:1",
                file.name()
            ));
        }
    }
    Ok(())
}

fn docx_expansion_ratio_exceeded(expanded: u64, compressed: u64) -> bool {
    expanded > compressed.saturating_mul(MAX_DOCX_EXPANSION_RATIO)
}

/// Read at most `MAX_DOCX_EXPANDED_BYTES`; this does not call `read_to_string`.
pub fn read_docx_xml_bounded<R: Read>(reader: &mut R, started: Instant) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        ensure_docx_time(started)?;
        let read = reader
            .read(&mut chunk)
            .map_err(|e| format!("read document.xml: {e}"))?;
        if read == 0 {
            break;
        }
        if bytes.len().saturating_add(read) > MAX_DOCX_EXPANDED_BYTES as usize {
            return Err(format!(
                "docx_expanded_limit_exceeded: document.xml exceeds {MAX_DOCX_EXPANDED_BYTES} bytes"
            ));
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_limits_refuse_values_before_allocation() {
        assert!(validate_rfc822_size(MAX_RFC822_MESSAGE_BYTES).is_ok());
        assert!(validate_rfc822_size(MAX_RFC822_MESSAGE_BYTES + 1).is_err());
        assert!(validate_attachment_size(MAX_ATTACHMENT_BYTES).is_ok());
        assert!(validate_attachment_size(MAX_ATTACHMENT_BYTES + 1).is_err());
    }

    #[test]
    fn unsafe_attachment_names_are_normalized_to_one_basename() {
        assert_eq!(normalize_attachment_filename("../../etc/passwd"), "passwd");
        assert_eq!(
            normalize_attachment_filename("C:\\temp\\evil.txt"),
            "evil.txt"
        );
        assert_eq!(normalize_attachment_filename("\0..\\"), "attachment.bin");
    }

    #[test]
    fn docx_expansion_ratio_uses_an_exact_bound() {
        assert!(!docx_expansion_ratio_exceeded(200, 2));
        assert!(docx_expansion_ratio_exceeded(201, 2));
    }

    #[test]
    fn malformed_content_types_fall_back_without_panicking() {
        assert_eq!(
            normalize_content_type("IMAGE/PNG; charset=binary"),
            "image/png"
        );
        assert_eq!(
            normalize_content_type("image/svg+xml\r\nX: yes"),
            "application/octet-stream"
        );
        assert_eq!(
            normalize_content_type("not a mime"),
            "application/octet-stream"
        );
    }
}
