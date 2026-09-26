//! Upstream response framing for the forward dispatch (issue #599 R2f.3).
//!
//! Framing used to come from the upstream `Content-Type` alone. A
//! Responses-native upstream — the ChatGPT Codex subscription backend in
//! particular — can stream `data:`/`event:` framing under a missing or generic
//! content type; the body then took the buffered JSON forwarder, which parsed
//! the event stream as one JSON document, dropped the terminal
//! `response.completed.response.usage`, and handed the client event bytes under
//! the wrong framing. The header stays authoritative when it names
//! `text/event-stream`; every other response is classified from a bounded
//! inspection of its actual body prefix, and the prefix is replayed ahead of
//! the untouched remainder so no byte is lost and nothing is fully buffered.

use anyhow::Context;
use bytes::Bytes;
use futures::{StreamExt, stream};

/// Maximum number of body bytes inspected to recognize event framing.
const SNIFF_LIMIT: usize = 1024;

/// SSE field names that prove event framing at the start of a body.
const SSE_FIELDS: [&[u8]; 4] = [b"data:", b"event:", b"id:", b"retry:"];

/// The framing ferry expects on one upstream response body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResponseKind {
    /// `data:`/`event:` event framing (`text/event-stream`).
    EventStream,
    /// A single JSON document.
    Json,
}

impl ResponseKind {
    pub(super) fn is_event_stream(self) -> bool {
        matches!(self, Self::EventStream)
    }
}

/// A response whose body prefix has been classified.
pub(super) struct SniffedResponse {
    pub(super) kind: ResponseKind,
    pub(super) response: reqwest::Response,
}

/// Read a bounded body prefix, classify the framing from those exact bytes, and
/// rebuild the response so the prefix and the untouched remainder are delivered
/// in order. The peek stops as soon as the prefix is decisive, so a live event
/// stream does not wait for the full bound.
pub(super) async fn sniff_response(response: reqwest::Response) -> anyhow::Result<SniffedResponse> {
    let status = response.status();
    let headers = response.headers().clone();
    let mut body_stream = response.bytes_stream();
    let mut prefix = Vec::with_capacity(SNIFF_LIMIT);
    let mut pending_error = None;
    while prefix.len() < SNIFF_LIMIT {
        match body_stream.next().await {
            Some(Ok(chunk)) => prefix.extend_from_slice(&chunk),
            Some(Err(err)) => {
                pending_error = Some(err);
                break;
            }
            None => break,
        }
        if classify_prefix(&prefix).is_some() {
            break;
        }
    }
    let kind = classify_prefix(&prefix).unwrap_or(ResponseKind::Json);
    let mut head: Vec<Result<Bytes, reqwest::Error>> = vec![Ok(Bytes::from(prefix))];
    if let Some(err) = pending_error {
        head.push(Err(err));
    }
    let body = reqwest::Body::wrap_stream(stream::iter(head).chain(body_stream));
    let mut builder = http::Response::builder().status(status);
    for (name, value) in headers.iter() {
        if name == http::header::CONTENT_LENGTH || name == http::header::TRANSFER_ENCODING {
            continue;
        }
        builder = builder.header(name.clone(), value.clone());
    }
    let response = builder
        .body(body)
        .context("failed to rebuild the sniffed upstream response")?
        .into();
    Ok(SniffedResponse { kind, response })
}

/// Classify event framing from a body prefix. `None` means the prefix is not
/// yet decisive — a field marker split across chunks — and more bytes are
/// needed before the caller can give up and treat the body as JSON.
fn classify_prefix(prefix: &[u8]) -> Option<ResponseKind> {
    let mut offset = if prefix.starts_with(&[0xEF, 0xBB, 0xBF]) {
        3
    } else {
        0
    };
    loop {
        match prefix.get(offset).copied() {
            None => return None,
            Some(b'\r' | b'\n') => offset += 1,
            Some(b':') => {
                let relative = prefix[offset..].iter().position(|byte| *byte == b'\n')?;
                offset += relative + 1;
            }
            Some(_) => {
                let rest = &prefix[offset..];
                return if rest.starts_with(b"{") || rest.starts_with(b"[") {
                    Some(ResponseKind::Json)
                } else if SSE_FIELDS.iter().any(|field| rest.starts_with(field)) {
                    Some(ResponseKind::EventStream)
                } else if SSE_FIELDS.iter().any(|field| field.starts_with(rest)) {
                    None
                } else {
                    Some(ResponseKind::Json)
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ResponseKind, classify_prefix, sniff_response};
    use bytes::Bytes;
    use futures::stream;

    #[test]
    fn classifies_event_framing() {
        assert_eq!(
            classify_prefix(b"data: {}\n\n"),
            Some(ResponseKind::EventStream)
        );
        assert_eq!(
            classify_prefix(b"event: response.created\n"),
            Some(ResponseKind::EventStream)
        );
        assert_eq!(
            classify_prefix(b": keep-alive\ndata: {}\n"),
            Some(ResponseKind::EventStream)
        );
        assert_eq!(
            classify_prefix(b"\xEF\xBB\xBFdata: {}\n"),
            Some(ResponseKind::EventStream)
        );
        assert_eq!(
            classify_prefix(b"\r\n\r\ndata: {}\r\n"),
            Some(ResponseKind::EventStream)
        );
    }

    #[test]
    fn classifies_json_documents() {
        assert_eq!(
            classify_prefix(b"{\"id\":\"resp_1\"}"),
            Some(ResponseKind::Json)
        );
        assert_eq!(classify_prefix(b" [1,2]"), Some(ResponseKind::Json));
        assert_eq!(classify_prefix(b"<html>"), Some(ResponseKind::Json));
    }

    #[test]
    fn undecided_on_split_field_markers() {
        assert_eq!(classify_prefix(b"da"), None);
        assert_eq!(classify_prefix(b"even"), None);
        assert_eq!(classify_prefix(b": keep-alive"), None);
        assert_eq!(classify_prefix(b""), None);
    }

    #[tokio::test]
    async fn sniff_replays_prefix_and_tail_in_order() {
        let body = b"data: {\"type\":\"response.completed\"}\n\n";
        let chunks = vec![
            Ok::<Bytes, reqwest::Error>(Bytes::from_static(b"da")),
            Ok(Bytes::from_static(
                b"ta: {\"type\":\"response.completed\"}\n\n",
            )),
        ];
        let response: reqwest::Response = http::Response::builder()
            .status(200)
            .header(http::header::CONTENT_TYPE, "application/octet-stream")
            .body(reqwest::Body::wrap_stream(stream::iter(chunks)))
            .expect("synthetic response")
            .into();
        let sniffed = sniff_response(response).await.expect("sniff");
        assert_eq!(sniffed.kind, ResponseKind::EventStream);
        let collected = sniffed.response.bytes().await.expect("body");
        assert_eq!(collected.as_ref(), body);
    }
}
