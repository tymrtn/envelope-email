// Copyright (c) 2026 Tyler Martin
// Licensed under FSL-1.1-ALv2 (see LICENSE)

//! Attachment download handler.

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::state::AppState;

#[derive(Deserialize)]
pub struct DownloadQuery {
    #[serde(default = "default_folder")]
    pub folder: String,
    #[serde(default)]
    pub inline: bool,
}

fn default_folder() -> String {
    "INBOX".to_string()
}

pub(crate) fn is_safe_inline_raster(content_type: &str, bytes: &[u8]) -> bool {
    match envelope_email_transport::ingress::normalize_content_type(content_type).as_str() {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "image/webp" => bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        _ => false,
    }
}

pub(crate) fn attachment_disposition(
    filename: &str,
    inline: bool,
    content_type: &str,
    bytes: &[u8],
) -> String {
    let disposition = if inline && is_safe_inline_raster(content_type, bytes) {
        "inline"
    } else {
        "attachment"
    };
    let safe_filename = envelope_email_transport::ingress::normalize_attachment_filename(filename);
    format!("{disposition}; filename=\"{safe_filename}\"")
}

pub async fn download(
    State(state): State<AppState>,
    Path((account_id, uid, filename)): Path<(String, u32, String)>,
    Query(q): Query<DownloadQuery>,
) -> Response {
    let (client_arc, _creds) = match state.get_or_create_imap(&account_id).await {
        Ok(c) => c,
        Err(e) => {
            return (StatusCode::BAD_GATEWAY, format!("IMAP: {e}")).into_response();
        }
    };
    let mut client = client_arc.lock().await;

    match envelope_email_transport::imap::download_attachment(
        &mut client,
        uid,
        &filename,
        &q.folder,
    )
    .await
    {
        Ok((fname, data)) => {
            let guessed_content_type = mime_guess::from_path(&fname)
                .first_or_octet_stream()
                .to_string();
            let content_type =
                envelope_email_transport::ingress::normalize_content_type(&guessed_content_type);
            Response::builder()
                .header(header::CONTENT_TYPE, content_type.clone())
                .header("X-Content-Type-Options", "nosniff")
                .header(
                    header::CONTENT_DISPOSITION,
                    attachment_disposition(&fname, q.inline, &content_type, &data),
                )
                .body(Body::from(data))
                .unwrap()
        }
        Err(e) => {
            state.evict_imap(&account_id).await;
            (StatusCode::BAD_GATEWAY, format!("download: {e}")).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_disposition_only_inlines_validated_raster_bytes() {
        assert_eq!(
            attachment_disposition("report.pdf", true, "application/pdf", b"%PDF"),
            "attachment; filename=\"report.pdf\""
        );
        assert_eq!(
            attachment_disposition("logo.png", true, "image/png", b"\x89PNG\r\n\x1a\nbody"),
            "inline; filename=\"logo.png\""
        );
        assert_eq!(
            attachment_disposition("vector.svg", true, "image/svg+xml", b"<svg></svg>"),
            "attachment; filename=\"vector.svg\""
        );
        assert_eq!(
            attachment_disposition("fake.png", true, "image/png", b"<svg></svg>"),
            "attachment; filename=\"fake.png\""
        );
        assert_eq!(
            attachment_disposition("../bad\"name.png", true, "image/png", b"\x89PNG\r\n\x1a\n"),
            "inline; filename=\"bad_name.png\""
        );
    }
}
