use crate::auth::error_response;
use axum::{
    body::Body,
    extract::Request,
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::Response,
};
use futures::TryStreamExt;
use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};

#[derive(Clone, Debug, Default)]
pub(crate) struct HttpRequestCompressionContext {
    pub(crate) content_encoding: Option<String>,
    pub(crate) compressed: bool,
    pub(crate) compressed_bytes: Option<i64>,
    pub(crate) compressed_bytes_counter: Option<Arc<AtomicI64>>,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct HttpRequestTransferStats {
    pub(crate) compressed_bytes: Option<i64>,
    pub(crate) decompressed_bytes: Option<i64>,
    pub(crate) compression_ratio: Option<f64>,
}

impl HttpRequestCompressionContext {
    pub(crate) fn from_headers(headers: &HeaderMap) -> Result<Self, Box<Response>> {
        let content_encoding = headers
            .get(header::CONTENT_ENCODING)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase());

        match content_encoding.as_deref() {
            None => Ok(Self {
                compressed_bytes: parse_content_length(headers),
                ..Self::default()
            }),
            Some("gzip") => Ok(Self {
                content_encoding,
                compressed: true,
                compressed_bytes: parse_content_length(headers),
                compressed_bytes_counter: None,
            }),
            Some(other) => Err(Box::new(error_response(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported_content_encoding",
                &format!("unsupported Content-Encoding: {other}"),
            ))),
        }
    }

    pub(crate) fn with_counter(mut self, counter: Arc<AtomicI64>) -> Self {
        self.compressed_bytes_counter = Some(counter);
        self
    }

    pub(crate) fn final_stats(&self, decompressed_bytes: i64) -> HttpRequestTransferStats {
        let compressed_bytes = self.compressed_bytes.or_else(|| {
            self.compressed_bytes_counter
                .as_ref()
                .map(|counter| counter.load(Ordering::Relaxed))
        });
        let compression_ratio = if self.compressed {
            compressed_bytes
                .filter(|value| *value > 0)
                .map(|value| decompressed_bytes as f64 / value as f64)
        } else {
            None
        };
        HttpRequestTransferStats {
            compressed_bytes,
            decompressed_bytes: Some(decompressed_bytes),
            compression_ratio,
        }
    }
}

fn parse_content_length(headers: &HeaderMap) -> Option<i64> {
    headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .and_then(|value| i64::try_from(value).ok())
}

pub(crate) async fn capture_request_compression(mut request: Request, next: Next) -> Response {
    let mut compression = match HttpRequestCompressionContext::from_headers(request.headers()) {
        Ok(compression) => compression,
        Err(response) => return *response,
    };

    if compression.compressed && compression.compressed_bytes.is_none() {
        let counter = Arc::new(AtomicI64::new(0));
        let (parts, body) = request.into_parts();
        let stream = body.into_data_stream().inspect_ok({
            let counter = counter.clone();
            move |chunk| {
                let len = i64::try_from(chunk.len()).unwrap_or(i64::MAX);
                counter.fetch_add(len, Ordering::Relaxed);
            }
        });
        request = Request::from_parts(parts, Body::from_stream(stream));
        compression = compression.with_counter(counter);
    }

    request.extensions_mut().insert(compression);
    next.run(request).await
}
