//! Serving of the frontend from the admin listener.
//!
//! The default binary embeds the SPA; runtime-only builds serve it from disk.
//! [`FRONTEND_DIST_ENV`] overrides the historical filesystem locations.
//!
//! Hashed assets get long-lived cache headers because their names change with
//! their contents; the SPA entry is served fresh so a redeploy is picked up on
//! the next reload.

use axum::Router;
#[cfg(any(feature = "embedded-frontend", test))]
use axum::body::Body;
#[cfg(feature = "embedded-frontend")]
use axum::extract::Path;
use axum::extract::Request;
#[cfg(any(feature = "embedded-frontend", test))]
use axum::http::HeaderMap;
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
#[cfg(any(feature = "embedded-frontend", test))]
use axum::response::IntoResponse;
use axum::response::Response;
#[cfg(feature = "embedded-frontend")]
use axum::routing::get;
use std::path::PathBuf;
use tower_http::services::{ServeDir, ServeFile};

#[cfg(feature = "embedded-frontend")]
use crate::web_assets;

/// Environment variable pointing at a filesystem frontend `dist` directory.
pub const FRONTEND_DIST_ENV: &str = "PROMPT_FERRY_FRONTEND_DIST";

/// Which frontend the admin router should serve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frontend {
    /// The SPA compiled into the binary.
    #[cfg(feature = "embedded-frontend")]
    Embedded,
    /// A filesystem `dist` directory; the deliberate development override.
    Filesystem(PathBuf),
}

/// Resolve the frontend to serve.
///
/// An explicit [`FRONTEND_DIST_ENV`] value always wins so an operator can
/// point a release binary at a different UI; otherwise the historical
/// locations are used when they contain a built `index.html`, which keeps a
/// development checkout with a fresh build on the filesystem path, and a
/// packaged binary without a directory beside it on the embedded path.
pub fn frontend() -> Frontend {
    match override_dist_dir() {
        Some(dist) => Frontend::Filesystem(dist),
        #[cfg(feature = "embedded-frontend")]
        None => Frontend::Embedded,
        #[cfg(not(feature = "embedded-frontend"))]
        None => Frontend::Filesystem(PathBuf::from("/app/frontend/dist")),
    }
}

/// The `Cache-Control` value for a hashed asset. The one-year lifetime is
/// spelled out in the header; the tests keep the wording intentional.
#[cfg(test)]
pub(crate) const IMMUTABLE_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";
#[cfg(not(test))]
const IMMUTABLE_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";

/// A frontend file ready to serve: the body plus the headers a browser needs.
#[cfg(any(feature = "embedded-frontend", test))]
struct FrontendFile {
    body: &'static [u8],
    content_type: &'static str,
    cache: CacheControl,
}

#[cfg(any(feature = "embedded-frontend", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CacheControl {
    /// Hashed asset: immutable, cacheable for a year.
    #[cfg(feature = "embedded-frontend")]
    Immutable,
    /// `index.html`: revalidate on every load so a redeploy is picked up.
    NoCache,
}

#[cfg(any(feature = "embedded-frontend", test))]
impl FrontendFile {
    #[cfg(feature = "embedded-frontend")]
    fn from_path(path: &str) -> Option<Self> {
        let body = web_assets::embedded_file(path)?;
        // Only files below the asset root may carry the immutable policy; the
        // entry point is always revalidated so a redeploy is seen.
        let cache = if path.starts_with("assets/") {
            CacheControl::Immutable
        } else {
            CacheControl::NoCache
        };
        Some(Self {
            body,
            content_type: content_type_for(path),
            cache,
        })
    }

    fn response(self) -> Response {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(self.content_type),
        );
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(match self.cache {
                #[cfg(feature = "embedded-frontend")]
                CacheControl::Immutable => IMMUTABLE_CACHE_CONTROL,
                CacheControl::NoCache => "no-cache",
            }),
        );
        (StatusCode::OK, headers, self.body).into_response()
    }
}

/// Serve `index.html` from the embedded build, or the fallback page that
/// explains a binary compiled without a frontend build.
#[cfg(feature = "embedded-frontend")]
pub(crate) async fn serve_embedded_index() -> Response {
    FrontendFile {
        body: web_assets::embedded_index(),
        content_type: "text/html; charset=utf-8",
        cache: CacheControl::NoCache,
    }
    .response()
}

/// Serve one embedded frontend file, or `404` when the build does not have it.
#[cfg(feature = "embedded-frontend")]
pub(crate) async fn serve_embedded_file(path: &str) -> Response {
    match FrontendFile::from_path(path) {
        Some(file) => file.response(),
        None => not_found(),
    }
}

/// Resolve the filesystem frontend directory, honouring the explicit override
/// first and then the historical locations that contain a built index.
fn override_dist_dir() -> Option<PathBuf> {
    if let Ok(path) = std::env::var(FRONTEND_DIST_ENV)
        && !path.trim().is_empty()
    {
        return Some(PathBuf::from(path));
    }
    ["/app/frontend/dist", "frontend/dist"]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.join("index.html").is_file())
}

#[cfg(any(feature = "embedded-frontend", test))]
fn not_found() -> Response {
    let mut response = (
        StatusCode::NOT_FOUND,
        [("content-type", "text/plain; charset=utf-8")],
        Body::from("frontend asset not found\n"),
    )
        .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}

/// Content type for an embedded frontend path, defaulting to
/// `application/octet-stream` so browsers never sniff an unknown body.
#[cfg(any(feature = "embedded-frontend", test))]
fn content_type_for(path: &str) -> &'static str {
    let extension = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match extension.as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// The frontend routes a management listener mounts beside its own API.
///
/// Both the worker's admin listener and the relay's management listener serve
/// the same build, so the routes live here: the hashed assets with their
/// long-lived cache policy, the favicon, and the entry point every browser path
/// falls back to. The caller mounts its API ahead of these, which is what keeps
/// an API path from resolving to the page.
///
/// The state parameter is what lets this merge into a router that already
/// resolved its own state.
pub fn frontend_routes<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    match frontend() {
        #[cfg(feature = "embedded-frontend")]
        Frontend::Embedded => embedded_routes(),
        Frontend::Filesystem(dist) => filesystem_routes(dist),
    }
}

#[cfg(feature = "embedded-frontend")]
fn embedded_routes<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route_service(
            "/assets/{*rest}",
            get(|Path(asset_path): Path<String>| async move {
                serve_embedded_file(&format!("assets/{asset_path}")).await
            }),
        )
        .route_service(
            "/favicon.svg",
            get(|| async { serve_embedded_file("favicon.svg").await }),
        )
        .fallback(get(serve_embedded_index))
}

fn filesystem_routes<S>(dist: PathBuf) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .nest_service("/assets", ServeDir::new(dist.join("assets")))
        .route_service("/favicon.svg", ServeFile::new(dist.join("favicon.svg")))
        .fallback_service(ServeFile::new(dist.join("index.html")))
        .layer(middleware::from_fn(filesystem_cache))
}

fn filesystem_cache_control(path: &str, status: StatusCode) -> &'static str {
    if status.is_success() && path.starts_with("/assets/") {
        IMMUTABLE_CACHE_CONTROL
    } else {
        "no-cache"
    }
}

async fn filesystem_cache(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    let mut response = next.run(request).await;
    let cache = filesystem_cache_control(&path, response.status());
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    response
}

#[cfg(test)]
mod tests {
    use super::{
        CacheControl, FrontendFile, IMMUTABLE_CACHE_CONTROL, content_type_for,
        filesystem_cache_control, not_found,
    };
    use axum::http::StatusCode;
    use axum::http::header;

    #[test]
    fn the_immutable_policy_covers_one_year_and_blocks_downgrade() {
        assert_eq!(
            IMMUTABLE_CACHE_CONTROL,
            "public, max-age=31536000, immutable"
        );
    }

    #[test]
    fn the_entry_point_is_never_given_the_immutable_policy() {
        let entry = FrontendFile {
            body: b"<html></html>",
            content_type: "text/html; charset=utf-8",
            cache: CacheControl::NoCache,
        }
        .response();
        assert_eq!(entry.headers()[header::CACHE_CONTROL], "no-cache");
    }

    #[test]
    fn filesystem_cache_matches_asset_and_document_policy() {
        assert_eq!(
            filesystem_cache_control("/assets/app.js", StatusCode::OK),
            IMMUTABLE_CACHE_CONTROL
        );
        assert_eq!(
            filesystem_cache_control("/assets/missing.js", StatusCode::NOT_FOUND),
            "no-cache"
        );
        assert_eq!(
            filesystem_cache_control("/assets-other", StatusCode::OK),
            "no-cache"
        );
        assert_eq!(
            filesystem_cache_control("/settings", StatusCode::OK),
            "no-cache"
        );
        assert_eq!(
            filesystem_cache_control("/index.html", StatusCode::OK),
            "no-cache"
        );
    }

    #[test]
    fn content_types_cover_the_built_asset_kinds() {
        for (path, expected) in [
            ("index.html", "text/html; charset=utf-8"),
            ("assets/app.js", "text/javascript; charset=utf-8"),
            ("assets/app.mjs", "text/javascript; charset=utf-8"),
            ("assets/app.css", "text/css; charset=utf-8"),
            ("assets/data.json", "application/json"),
            ("assets/sourcemap.js.map", "application/json"),
            ("favicon.svg", "image/svg+xml"),
            ("assets/logo.png", "image/png"),
            ("assets/logo.webp", "image/webp"),
            ("assets/logo.avif", "image/avif"),
            ("favicon.ico", "image/x-icon"),
            ("assets/font.woff2", "font/woff2"),
            ("assets/font.woff", "font/woff"),
            ("assets/font.ttf", "font/ttf"),
            ("assets/font.otf", "font/otf"),
            ("assets/module.wasm", "application/wasm"),
            ("assets/notes.txt", "text/plain; charset=utf-8"),
            ("assets/blob.bin", "application/octet-stream"),
            ("assets/noextension", "application/octet-stream"),
        ] {
            assert_eq!(content_type_for(path), expected, "content type for {path}");
        }
    }

    #[test]
    fn an_unknown_embedded_file_is_a_404_without_cache() {
        let response = not_found();
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
    }
}
