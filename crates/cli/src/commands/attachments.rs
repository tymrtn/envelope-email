// Copyright (c) 2026 Tyler Martin
// Licensed under FSL-1.1-ALv2 (see LICENSE)

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use envelope_email_store::CredentialBackend;
use envelope_email_transport::smtp::Attachment;

use super::common::setup_credentials;

/// Read each `--attach` file and snapshot its bytes into a JSON attachment
/// entry suitable for persisting on a draft.
///
/// Each entry carries `filename`, `content_type`, `size`, and a base64
/// `data_base64` payload. Returns an explicit error if any file cannot be read
/// so a draft is never created with a silently-missing attachment. This is the
/// same snapshot convention used by scheduled sends.
pub(crate) fn snapshot_attachments(attach_paths: &[String]) -> Result<Vec<serde_json::Value>> {
    use base64::Engine as _;
    let mut out = Vec::with_capacity(attach_paths.len());
    for path_str in attach_paths {
        let path = std::path::Path::new(path_str);
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("attachment")
            .to_string();
        let data = std::fs::read(path)
            .with_context(|| format!("failed to read attachment: {path_str}"))?;
        let content_type = mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string();
        let data_base64 = base64::engine::general_purpose::STANDARD.encode(&data);
        out.push(serde_json::json!({
            "filename": filename,
            "content_type": content_type,
            "size": data.len(),
            "data_base64": data_base64,
        }));
    }
    Ok(out)
}

/// Build a non-secret summary (filename, content_type, size) of stored draft
/// attachments. Deliberately excludes `data_base64` so attachment bytes never
/// appear in command output, logs, or audit surfaces.
pub(crate) fn attachment_summaries(attachments: &[serde_json::Value]) -> Vec<serde_json::Value> {
    attachments
        .iter()
        .map(|a| {
            serde_json::json!({
                "filename": a.get("filename").cloned().unwrap_or(serde_json::Value::Null),
                "content_type": a.get("content_type").cloned().unwrap_or(serde_json::Value::Null),
                "size": a.get("size").cloned().unwrap_or(serde_json::Value::Null),
            })
        })
        .collect()
}

/// Decode snapshotted draft attachment JSON entries back into transport
/// [`Attachment`]s with their original bytes.
///
/// Returns an error if any entry is missing its `data_base64` payload or fails
/// to decode, so the caller can refuse to send rather than silently dropping
/// the attachment.
pub(crate) fn decode_attachments(attachments: &[serde_json::Value]) -> Result<Vec<Attachment>> {
    use base64::Engine as _;
    let mut out = Vec::with_capacity(attachments.len());
    for entry in attachments {
        let filename = entry
            .get("filename")
            .and_then(|v| v.as_str())
            .unwrap_or("attachment")
            .to_string();
        let content_type = entry
            .get("content_type")
            .and_then(|v| v.as_str())
            .unwrap_or("application/octet-stream")
            .to_string();
        let data_b64 = entry
            .get("data_base64")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("attachment '{filename}' has no data_base64 payload"))?;
        let data = base64::engine::general_purpose::STANDARD
            .decode(data_b64)
            .map_err(|e| anyhow::anyhow!("attachment '{filename}' base64 decode failed: {e}"))?;
        out.push(Attachment {
            filename,
            content_type,
            data,
        });
    }
    Ok(out)
}

/// Default directory for implicit attachment downloads. An attachment-controlled
/// filename is always reduced to a basename under this directory.
const DEFAULT_DOWNLOAD_DIR: &str = "envelope-downloads";

fn reject_symlink_components(path: &Path) -> Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if let Ok(meta) = fs::symlink_metadata(&current)
            && meta.file_type().is_symlink()
        {
            bail!(
                "refusing attachment output through symlink: {}",
                current.display()
            );
        }
    }
    Ok(())
}

fn implicit_download_path(filename: &str) -> Result<PathBuf> {
    let base =
        std::env::current_dir().context("resolve current directory for attachment download")?;
    implicit_download_path_from(&base, filename)
}

fn implicit_download_path_from(base: &Path, filename: &str) -> Result<PathBuf> {
    reject_symlink_components(base)?;
    let root = base.join(DEFAULT_DOWNLOAD_DIR);
    // Do not use create_dir_all here: it follows an existing leaf symlink
    // before we get a chance to reject it. The implicit root is one known child
    // of an existing operator-selected working directory.
    match fs::create_dir(&root) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(error).with_context(|| format!("create download root {}", root.display()));
        }
    }
    reject_symlink_components(&root)?;
    let meta =
        fs::metadata(&root).with_context(|| format!("inspect download root {}", root.display()))?;
    if !meta.is_dir() {
        bail!(
            "attachment download root is not a directory: {}",
            root.display()
        );
    }
    let basename = envelope_email_transport::ingress::normalize_attachment_filename(filename);
    Ok(root.join(basename))
}

fn explicit_download_path(output: &str) -> Result<PathBuf> {
    let path = PathBuf::from(output);
    if output.is_empty() || path.file_name().is_none() {
        bail!("--output must name a file");
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    reject_symlink_components(parent)?;
    let meta = fs::metadata(parent)
        .with_context(|| format!("--output parent does not exist: {}", parent.display()))?;
    if !meta.is_dir() {
        bail!("--output parent is not a directory: {}", parent.display());
    }
    Ok(path)
}

/// Create a new output file only. This intentionally never overwrites a local
/// file and refuses both a symlink target and a symlinked parent component.
fn write_new_download(path: &Path, bytes: &[u8]) -> Result<()> {
    reject_symlink_components(path.parent().unwrap_or_else(|| Path::new(".")))?;
    if let Ok(meta) = fs::symlink_metadata(path)
        && meta.file_type().is_symlink()
    {
        bail!("refusing attachment output symlink: {}", path.display());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("refusing to overwrite attachment output {}", path.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("write attachment output {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("sync attachment output {}", path.display()))?;
    Ok(())
}

/// List attachments for a message by UID.
#[tokio::main]
pub async fn run_list(
    uid: u32,
    folder: &str,
    account: Option<&str>,
    json: bool,
    backend: CredentialBackend,
) -> Result<()> {
    let (_db, creds) = setup_credentials(account, backend)?;

    let mut client = envelope_email_transport::imap::connect(&creds)
        .await
        .context("IMAP connection failed")?;

    let message = envelope_email_transport::imap::fetch_message(&mut client, folder, uid).await?;

    match message {
        Some(msg) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&msg.attachments)?);
            } else if msg.attachments.is_empty() {
                println!("No attachments for UID {uid} in {folder}");
            } else {
                println!("Attachments for UID {uid}:");
                for (i, att) in msg.attachments.iter().enumerate() {
                    println!(
                        "  {i}: {name}  ({ct}, {size} bytes)",
                        name = att.filename,
                        ct = att.content_type,
                        size = att.size,
                    );
                }
            }
        }
        None => bail!("message UID {uid} not found in {folder}"),
    }

    Ok(())
}

/// Download an attachment by filename from a message, saving to disk.
#[tokio::main]
pub async fn run_download(
    uid: u32,
    filename: &str,
    output: Option<&str>,
    folder: &str,
    account: Option<&str>,
    json: bool,
    backend: CredentialBackend,
) -> Result<()> {
    let (_db, creds) = setup_credentials(account, backend)?;

    let mut client = envelope_email_transport::imap::connect(&creds)
        .await
        .context("IMAP connection failed")?;

    let (name, bytes) =
        envelope_email_transport::imap::download_attachment(&mut client, uid, filename, folder)
            .await
            .context("failed to download attachment")?;

    // An implicit destination is always a sanitized basename under a dedicated
    // root. `--output` is an explicit operator choice but still gets no-symlink,
    // create-new semantics so it cannot overwrite or follow an existing link.
    let dest = match output {
        Some(p) => explicit_download_path(p)?,
        None => implicit_download_path(&name)?,
    };
    write_new_download(&dest, &bytes)?;

    if json {
        let info = serde_json::json!({
            "filename": name,
            "size": bytes.len(),
            "path": dest.display().to_string(),
        });
        println!("{}", serde_json::to_string_pretty(&info)?);
    } else {
        println!(
            "Saved {name} ({size} bytes) to {path}",
            size = bytes.len(),
            path = dest.display(),
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn implicit_attachment_filename_normalizes_traversal_and_controls() {
        let normalized = envelope_email_transport::ingress::normalize_attachment_filename(
            "../../tmp/evil\0.pdf",
        );
        assert_eq!(normalized, "evil.pdf");
        assert!(!Path::new(&normalized).is_absolute());
        assert_eq!(Path::new(&normalized).components().count(), 1);
    }

    #[test]
    fn download_write_refuses_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("existing.txt");
        fs::write(&path, b"original").unwrap();
        assert!(write_new_download(&path, b"replacement").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"original");
    }

    #[cfg(unix)]
    #[test]
    fn implicit_download_refuses_a_symlinked_root_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = dir.path().join(DEFAULT_DOWNLOAD_DIR);
        std::os::unix::fs::symlink(outside.path(), &root).unwrap();

        assert!(implicit_download_path_from(dir.path(), "report.pdf").is_err());
        assert!(!outside.path().join("report.pdf").exists());
    }

    #[test]
    fn explicit_relative_output_uses_current_directory_parent() {
        assert_eq!(
            explicit_download_path("report.pdf").unwrap(),
            PathBuf::from("report.pdf")
        );
    }

    #[cfg(unix)]
    #[test]
    fn download_write_refuses_symlink_target() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        let link = dir.path().join("attachment.txt");
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        assert!(write_new_download(&link, b"payload").is_err());
        assert!(outside.as_file().metadata().unwrap().len() == 0);
    }

    #[cfg(unix)]
    #[test]
    fn download_write_refuses_symlinked_parent() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let link = dir.path().join("downloads");
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        let path = link.join("attachment.txt");
        assert!(write_new_download(&path, b"payload").is_err());
        assert!(!outside.path().join("attachment.txt").exists());
    }

    #[test]
    fn snapshot_attachments_encodes_bytes_and_metadata() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"hello").unwrap();
        let path = f.path().to_str().unwrap().to_string();

        let snap = snapshot_attachments(&[path]).unwrap();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0]["size"], 5);
        // "hello" base64-encoded
        assert_eq!(snap[0]["data_base64"], "aGVsbG8=");
        assert!(!snap[0]["filename"].as_str().unwrap().is_empty());
    }

    #[test]
    fn snapshot_attachments_errors_on_missing_file() {
        let err = snapshot_attachments(&["/no/such/path/at/all.txt".to_string()]).unwrap_err();
        assert!(err.to_string().contains("failed to read attachment"));
    }

    #[test]
    fn empty_attach_paths_snapshot_is_empty() {
        assert!(snapshot_attachments(&[]).unwrap().is_empty());
    }

    #[test]
    fn attachment_summaries_exclude_bytes() {
        let attachments = vec![serde_json::json!({
            "filename": "secret.txt",
            "content_type": "text/plain",
            "size": 5,
            "data_base64": "aGVsbG8=",
        })];
        let summary = attachment_summaries(&attachments);
        let serialized = serde_json::to_string(&summary).unwrap();
        assert!(!serialized.contains("data_base64"));
        assert!(!serialized.contains("aGVsbG8="));
        assert!(serialized.contains("secret.txt"));
        assert!(serialized.contains("text/plain"));
        assert_eq!(summary[0]["size"], 5);
    }

    #[test]
    fn decode_attachments_round_trips_snapshot() {
        let snap = vec![serde_json::json!({
            "filename": "packet.txt",
            "content_type": "text/plain",
            "size": 5,
            "data_base64": "aGVsbG8=",
        })];
        let decoded = decode_attachments(&snap).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].filename, "packet.txt");
        assert_eq!(decoded[0].content_type, "text/plain");
        assert_eq!(decoded[0].data, b"hello");
    }

    #[test]
    fn decode_attachments_errors_without_payload() {
        let snap = vec![serde_json::json!({
            "filename": "packet.txt",
            "content_type": "text/plain",
            "size": 5,
        })];
        let err = decode_attachments(&snap).unwrap_err();
        assert!(err.to_string().contains("no data_base64"));
    }
}
