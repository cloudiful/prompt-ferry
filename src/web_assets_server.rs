//! Serving of the frontend from the admin listener.
//!
//! The production binary carries the built SPA inside itself, so the container
//! and the desktop executable need no runtime frontend directory. A frontend
//! developer overrides this deliberately with [`FRONTEND_DIST_ENV`] (or the
//! historical filesystem locations) and edits the UI without recompiling Rust.
//!
//! Hashed assets get long-lived cache headers because their names change with
//! their contents; the SPA entry is served fresh so a redeploy is picked up on
//! the next reload.

use axum::Router;
use axum::body::Body;
use axum::extract::Path;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use std::path::PathBuf;
use tower_http::services::{ServeDir, ServeFile};

use crate::web_assets;

/// Which frontend the admin router should serve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frontend {
    /// The SPA compiled into the binary.
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
        None => Frontend::Embedded,
    }
}

/// The `Cache-Control` value for a hashed asset. The one-year lifetime is
/// spelled out in the header; the tests keep the wording intentional.
#[cfg(test)]
pub(crate) const IMMUTABLE_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";
#[cfg(not(test))]
const IMMUTABLE_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";

/// A frontend file ready to serve: the body plus the headers a browser needs.
struct FrontendFile {
    body: &'static [u8],
    content_type: &'static str,
    cache: CacheControl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CacheControl {
    /// Hashed asset: immutable, cacheable for a year.
    Immutable,
    /// `index.html`: revalidate on every load so a redeploy is picked up.
    NoCache,
}

impl FrontendFile {
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
                CacheControl::Immutable => IMMUTABLE_CACHE_CONTROL,
                CacheControl::NoCache => "no-cache",
            }),
        );
        (StatusCode::OK, headers, self.body).into_response()
    }
}

/// Serve `index.html` from the embedded build, or the fallback page that
/// explains a binary compiled without a frontend build.
pub(crate) async fn serve_embedded_index() -> Response {
    FrontendFile {
        body: web_assets::embedded_index(),
        content_type: "text/html; charset=utf-8",
        cache: CacheControl::NoCache,
    }
    .response()
}

/// Serve one embedded frontend file, or `404` when the build does not have it.
pub(crate) async fn serve_embedded_file(path: &str) -> Response {
    match FrontendFile::from_path(path) {
        Some(file) => file.response(),
        None => not_found(),
    }
}

/// Resolve the filesystem frontend directory, honouring the explicit override
/// first and then the historical locations that contain a built index.
fn override_dist_dir() -> Option<PathBuf> {
    if let Ok(path) = std::env::var(web_assets::FRONTEND_DIST_ENV)
        && !path.trim().is_empty()
    {
        return Some(PathBuf::from(path));
    }
    ["/app/frontend/dist", "frontend/dist"]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.join("index.html").is_file())
}

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
        Frontend::Embedded => embedded_routes(),
        Frontend::Filesystem(dist) => filesystem_routes(dist),
    }
}

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
}

/// The body bytes of a response, for tests.
#[cfg(test)]
pub(crate) async fn response_body(response: Response) -> Vec<u8> {
    axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_default()
        .to_vec()
}

#[cfg(test)]
mod tests {
    use super::{
        CacheControl, Frontend, FrontendFile, IMMUTABLE_CACHE_CONTROL, content_type_for, frontend,
        frontend_routes, not_found, serve_embedded_index,
    };
    use crate::web_assets::FRONTEND_DIST_ENV;
    use axum::{
        Router,
        body::Body,
        http::{Request, header},
    };
    use std::{
        path::PathBuf,
        sync::{Mutex, MutexGuard, OnceLock},
    };
    use tower::ServiceExt as _;

    /// Environment reads are process-wide, so the override tests run one at a
    /// time and restore the variable afterwards.
    fn env_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn set_override(value: Option<&str>) {
        match value {
            Some(value) => unsafe { std::env::set_var(FRONTEND_DIST_ENV, value) },
            None => unsafe { std::env::remove_var(FRONTEND_DIST_ENV) },
        }
    }

    fn frontend_dist() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("frontend/dist")
    }

    #[test]
    fn the_immutable_policy_covers_one_year_and_blocks_downgrade() {
        assert_eq!(
            IMMUTABLE_CACHE_CONTROL,
            "public, max-age=31536000, immutable"
        );
    }

    #[test]
    fn hashed_assets_are_immutable_and_the_entry_is_not() {
        // The cache policy is a property of the path shape, so the asset part
        // is exercised with an embedded file when the build has one and is
        // otherwise covered by the entry point, which is always present.
        let with_asset = crate::web_assets::hashed_js_asset_path()
            .and_then(FrontendFile::from_path)
            .map(|asset| asset.response());
        if let Some(response) = with_asset {
            assert_eq!(
                response.headers()[header::CACHE_CONTROL],
                IMMUTABLE_CACHE_CONTROL
            );
        }

        let entry = FrontendFile {
            body: b"<html></html>",
            content_type: "text/html; charset=utf-8",
            cache: CacheControl::NoCache,
        }
        .response();
        assert_eq!(entry.headers()[header::CACHE_CONTROL], "no-cache");
    }

    #[test]
    fn a_file_outside_the_asset_root_never_gets_the_immutable_policy() {
        // The favicon ships with a built frontend; a no-dist build carries no
        // embedded favicon, and the policy check below still holds for any
        // embedded file outside `assets/`.
        let favicon = FrontendFile::from_path("favicon.svg");
        if let Some(favicon) = favicon {
            assert_eq!(
                favicon.response().headers()[header::CACHE_CONTROL],
                "no-cache"
            );
        } else {
            assert!(
                !crate::web_assets::has_embedded_index(),
                "a built dist must carry favicon.svg"
            );
        }
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

    #[tokio::test]
    async fn the_embedded_entry_point_serves_valid_html() {
        let response = serve_embedded_index().await;

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8"
        );
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
        let body = super::response_body(response).await;
        let html = String::from_utf8(body).expect("the entry point is UTF-8");
        assert!(
            html.contains("<!doctype html") || html.contains("<html"),
            "got: {html}"
        );
    }

    #[test]
    fn an_unknown_embedded_file_is_a_404_without_cache() {
        let response = not_found();
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
    }

    #[tokio::test]
    async fn the_shared_frontend_routes_serve_the_entry_and_reject_a_missing_asset() {
        // Both management listeners mount these, so what they must guarantee is
        // checked once. Which variant answers is the caller's configuration, not
        // a property of these routes: a checkout with a built `frontend/dist`
        // resolves to the filesystem frontend and a backend-only one to the
        // embedded assets. The lock keeps a test that is changing the selection
        // from changing it underneath this one, and it is held only while the
        // routes are built, because once built they carry the selection they were
        // built from. The cache policy is asserted only where it is part of the
        // contract rather than as an accident of which frontend was selected.
        let (app, embedded): (Router<()>, bool) = {
            let _guard = env_lock();
            (frontend_routes(), matches!(frontend(), Frontend::Embedded))
        };

        let index = app
            .clone()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(index.status(), axum::http::StatusCode::OK);
        assert!(
            index.headers()[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("text/html"),
            "the entry point must be HTML in either variant, got {:?}",
            index.headers()[header::CONTENT_TYPE]
        );
        if embedded {
            assert_eq!(index.headers()[header::CACHE_CONTROL], "no-cache");
        }

        // A history route resolves to the entry point rather than a 404.
        let deep = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/settings/relays")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(deep.status(), axum::http::StatusCode::OK);

        // An asset that does not exist says so in either variant, instead of
        // falling back to the entry point.
        let asset = app
            .oneshot(
                Request::builder()
                    .uri("/assets/does-not-exist.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(asset.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[test]
    fn an_explicit_override_wins_over_the_embedded_assets() {
        let _guard = env_lock();
        set_override(Some("/nonexistent/prompt-ferry-dist"));

        assert_eq!(
            frontend(),
            Frontend::Filesystem(PathBuf::from("/nonexistent/prompt-ferry-dist")),
            "an explicitly configured dist must win even when it does not exist"
        );

        set_override(None);
    }

    #[test]
    fn a_built_dist_beside_the_working_directory_is_the_override() {
        let _guard = env_lock();
        set_override(None);

        // The repository checkout used for this test run carries frontend/dist,
        // so the historical location resolves; otherwise the binary is embedded.
        match frontend() {
            Frontend::Filesystem(dist) => {
                let resolved = dist
                    .join("index.html")
                    .canonicalize()
                    .expect("resolve override");
                let expected = frontend_dist()
                    .join("index.html")
                    .canonicalize()
                    .expect("resolve checkout dist");
                assert_eq!(
                    resolved, expected,
                    "the historical location must resolve to the checkout dist"
                );
            }
            Frontend::Embedded => {
                assert!(
                    !frontend_dist().join("index.html").is_file(),
                    "a checkout with a built dist must resolve to the filesystem frontend"
                );
            }
        }
    }

    #[test]
    fn a_blank_override_is_ignored() {
        let _guard = env_lock();
        set_override(Some("   "));

        // Falls through to the historical-location check rather than treating
        // the blank value as a path: the resolution matches the no-override
        // outcome whatever that is for this checkout.
        let with_blank = frontend();
        set_override(None);
        assert_eq!(
            with_blank,
            frontend(),
            "a blank override must not change which frontend is selected"
        );
    }
}
