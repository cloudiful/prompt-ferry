//! Shared fixture for the graceful-shutdown subprocess tests.
//!
//! Each fixture starts the real `prompt-ferry serve` binary in integrated mode
//! against a local loopback upstream, so the operating-system signal handler,
//! the worker bridge session, and the relay listeners are the production ones.
//! Only the upstream AI endpoint is a test double.

use std::{
    collections::VecDeque,
    io,
    net::SocketAddr,
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, StatusCode, header},
    response::Response,
    routing::post,
};
use bytes::Bytes;
use futures::{StreamExt, stream};
use tokio::{sync::Semaphore, task::JoinHandle};

pub const CLIENT_TOKEN: &str = "graceful-client-token";

const LOG_LINE_LIMIT: usize = 400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpstreamMode {
    Immediate,
    StreamingGated,
    GatedJson,
    NeverEnding,
}

impl UpstreamMode {
    pub fn header_value(self) -> &'static str {
        match self {
            Self::Immediate => "immediate",
            Self::StreamingGated => "streaming-gated",
            Self::GatedJson => "gated-json",
            Self::NeverEnding => "never-ending",
        }
    }
}

struct UpstreamControl {
    hits: Arc<AtomicUsize>,
    admitted: Arc<Semaphore>,
    release: Arc<Semaphore>,
}

pub struct Upstream {
    pub addr: SocketAddr,
    pub hits: Arc<AtomicUsize>,
    admitted: Arc<Semaphore>,
    release: Arc<Semaphore>,
    task: JoinHandle<()>,
}

impl Upstream {
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake upstream");
        let addr = listener.local_addr().expect("fake upstream addr");
        let control = Arc::new(UpstreamControl {
            hits: Arc::new(AtomicUsize::new(0)),
            admitted: Arc::new(Semaphore::new(0)),
            release: Arc::new(Semaphore::new(0)),
        });
        let app = Router::new()
            .route("/v1/chat/completions", post(upstream_handler))
            .with_state(control.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("fake upstream serves");
        });
        Self {
            addr,
            hits: control.hits.clone(),
            admitted: control.admitted.clone(),
            release: control.release.clone(),
            task,
        }
    }

    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }

    /// Wait until the gated upstream handler has admitted the request.
    pub async fn wait_admitted(&self) {
        self.admitted
            .acquire()
            .await
            .expect("admission semaphore open")
            .forget();
    }

    /// Let a gated upstream handler emit its response.
    pub fn release(&self) {
        self.release.add_permits(1);
    }
}

impl Drop for Upstream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn upstream_handler(
    State(control): State<Arc<UpstreamControl>>,
    headers: HeaderMap,
    request: Request,
) -> Response {
    control.hits.fetch_add(1, Ordering::SeqCst);
    let body = axum::body::to_bytes(request.into_body(), usize::MAX).await;
    // The relay/worker forwards the JSON body unchanged, so the requested
    // model identifies the fixture behaviour without depending on header
    // forwarding.
    let requested_model = body
        .ok()
        .and_then(|body| serde_json::from_slice::<serde_json::Value>(&body).ok())
        .and_then(|body| {
            body.get("model")
                .and_then(|model| model.as_str())
                .map(str::to_string)
        });
    let mode = requested_model.as_deref().unwrap_or_else(|| {
        headers
            .get("x-graceful-mode")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("immediate")
    });

    match mode {
        "streaming-gated" => {
            control.admitted.add_permits(1);
            let first = stream::once(async {
                Ok::<Bytes, io::Error>(Bytes::from_static(b"data: {\"delta\":\"hello\"}\n\n"))
            });
            let gate = stream::once(async move {
                let _ = control.release.acquire().await;
                Ok::<Bytes, io::Error>(Bytes::from_static(b"data: [DONE]\n\n"))
            });
            sse_response(first.chain(gate))
        }
        "gated-json" => {
            control.admitted.add_permits(1);
            let _ = control.release.acquire().await;
            json_response(r#"{"id":"response-1","object":"chat.completion"}"#)
        }
        "never-ending" => {
            control.admitted.add_permits(1);
            let first = stream::once(async {
                Ok::<Bytes, io::Error>(Bytes::from_static(b"data: {\"delta\":\"start\"}\n\n"))
            });
            let rest = stream::pending::<Result<Bytes, io::Error>>();
            sse_response(first.chain(rest))
        }
        _ => sse_response(stream_events()),
    }
}

fn stream_events() -> impl futures::Stream<Item = Result<Bytes, io::Error>> {
    stream::iter(vec![
        Ok::<Bytes, io::Error>(Bytes::from_static(b"data: {\"delta\":\"hello\"}\n\n")),
        Ok(Bytes::from_static(b"data: [DONE]\n\n")),
    ])
}

fn sse_response(
    body: impl futures::Stream<Item = Result<Bytes, io::Error>> + Send + 'static,
) -> Response {
    let mut response = Response::new(Body::from_stream(body));
    *response.status_mut() = StatusCode::OK;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, "text/event-stream".parse().unwrap());
    response
}

fn json_response(body: &'static str) -> Response {
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = StatusCode::OK;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    response
}

#[derive(Default)]
struct LogBuffer {
    lines: Mutex<VecDeque<String>>,
}

impl LogBuffer {
    fn push(&self, line: String) {
        let mut lines = self
            .lines
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if lines.len() == LOG_LINE_LIMIT {
            lines.pop_front();
        }
        lines.push_back(strip_ansi(&line));
    }

    fn snapshot(&self) -> Vec<String> {
        self.lines
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .cloned()
            .collect()
    }
}

fn strip_ansi(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        output.push(ch);
    }
    output
}

pub struct Server {
    pub public_addr: SocketAddr,
    child: Child,
    logs: Arc<LogBuffer>,
    _dir: TempDirGuard,
    exited: bool,
}

struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Server {
    pub async fn start(
        upstream: &Upstream,
        worker_drain_seconds: u64,
        relay_drain_seconds: u64,
    ) -> Self {
        let dir = unique_dir();
        let dir_guard = TempDirGuard(dir.clone());
        std::fs::create_dir_all(dir.join("data")).expect("create data dir");
        std::fs::create_dir_all(dir.join("config")).expect("create config dir");
        let dotenv = dir.join("empty.env");
        std::fs::write(&dotenv, "").expect("write empty dotenv");

        let mut command = Command::new(env!("CARGO_BIN_EXE_prompt-ferry"));
        command
            .arg("serve")
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("NO_COLOR", "1")
            .env("RUST_LOG", "info,rmcp::service=warn")
            .env("DATABASE_URL", "")
            .env("PROMPT_FERRY_DOTENV", &dotenv)
            .env("XDG_DATA_HOME", dir.join("data"))
            .env("XDG_CONFIG_HOME", dir.join("config"))
            .env("PROMPT_FERRY_RELAY__BIND", "127.0.0.1:0")
            .env("PROMPT_FERRY_RELAY__WORKER_BIND", "127.0.0.1:0")
            .env("PROMPT_FERRY_RELAY__ADMIN_BIND", "127.0.0.1:0")
            .env("PROMPT_FERRY_SERVE__INTERNAL_WORKER_BIND", "127.0.0.1:0")
            .env("PROMPT_FERRY_WORKER__ADMIN_BIND", "127.0.0.1:0")
            .env("PROMPT_FERRY_WORKER__DATABASE_URL", "")
            .env("PROMPT_FERRY_WORKER__VALKEY_URL", "")
            .env(
                "PROMPT_FERRY_WORKER__STANDALONE_DATABASE_PATH",
                dir.join("data").join("graceful.sqlite"),
            )
            .env(
                "PROMPT_FERRY_WORKER__UPSTREAM_BASE_URL",
                format!("http://{}", upstream.addr),
            )
            .env("PROMPT_FERRY_WORKER__UPSTREAM_API_KEY", "test-upstream-key")
            .env("PROMPT_FERRY_WORKER__UPSTREAM_NATIVE_API", "chat")
            .env("PROMPT_FERRY_WORKER__WORKER_TOKEN", "graceful-worker-token")
            .env("PROMPT_FERRY_WORKER__MCP_WARMUP", "off")
            .env(
                "PROMPT_FERRY_WORKER__SHUTDOWN_DRAIN_SECONDS",
                worker_drain_seconds.to_string(),
            )
            .env("PROMPT_FERRY_RELAY__CLIENT_TOKEN", CLIENT_TOKEN)
            .env("PROMPT_FERRY_RELAY__WORKER_TOKEN", "graceful-worker-token")
            .env(
                "PROMPT_FERRY_RELAY__SHUTDOWN_DRAIN_SECONDS",
                relay_drain_seconds.to_string(),
            );
        remove_inherited_config(&mut command);

        let mut child = command.spawn().expect("spawn prompt-ferry serve");
        let logs = Arc::new(LogBuffer::default());
        capture(child.stdout.take(), logs.clone(), "stdout");
        capture(child.stderr.take(), logs.clone(), "stderr");

        let public_addr = wait_for_public_addr(&mut child, &logs).await;
        Self {
            public_addr,
            child,
            logs,
            _dir: dir_guard,
            exited: false,
        }
    }

    pub fn logs(&self) -> Vec<String> {
        self.logs.snapshot()
    }

    pub fn log_contains(&self, needle: &str) -> bool {
        self.logs().iter().any(|line| line.contains(needle))
    }

    pub async fn wait_for_log(&self, needle: &str, timeout: Duration) {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if self.log_contains(needle) {
                return;
            }
            if tokio::time::Instant::now() >= deadline {
                panic!(
                    "log line `{needle}` did not appear within {timeout:?}; logs:\n{}",
                    self.logs().join("\n")
                );
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn signal_term(&self) {
        let status = Command::new("kill")
            .arg("-TERM")
            .arg(self.pid().to_string())
            .status()
            .expect("run kill");
        assert!(status.success(), "kill -TERM failed: {status}");
    }

    pub async fn wait_exit(&mut self, timeout: Duration) -> ExitStatus {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait().expect("poll child") {
                self.exited = true;
                return status;
            }
            if tokio::time::Instant::now() >= deadline {
                let logs = self.logs().join("\n");
                self.kill_and_reap();
                panic!("prompt-ferry did not exit within {timeout:?}; logs:\n{logs}");
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    fn kill_and_reap(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.exited = true;
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if !self.exited {
            self.kill_and_reap();
        }
    }
}

fn capture(
    stream: Option<impl io::Read + Send + 'static>,
    logs: Arc<LogBuffer>,
    label: &'static str,
) {
    let Some(stream) = stream else {
        return;
    };
    std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stream);
        for line in std::io::BufRead::lines(reader).map_while(Result::ok) {
            logs.push(format!("{label}: {line}"));
        }
    });
}

fn remove_inherited_config(command: &mut Command) {
    // The test runner may export application settings; the child must read only
    // the fixture values configured above plus its empty dotenv file.
    const FIXTURE_ENV_KEYS: &[&str] = &[
        "PROMPT_FERRY_DOTENV",
        "PROMPT_FERRY_RELAY__BIND",
        "PROMPT_FERRY_RELAY__WORKER_BIND",
        "PROMPT_FERRY_RELAY__ADMIN_BIND",
        "PROMPT_FERRY_RELAY__CLIENT_TOKEN",
        "PROMPT_FERRY_RELAY__WORKER_TOKEN",
        "PROMPT_FERRY_RELAY__SHUTDOWN_DRAIN_SECONDS",
        "PROMPT_FERRY_SERVE__INTERNAL_WORKER_BIND",
        "PROMPT_FERRY_WORKER__ADMIN_BIND",
        "PROMPT_FERRY_WORKER__DATABASE_URL",
        "PROMPT_FERRY_WORKER__VALKEY_URL",
        "PROMPT_FERRY_WORKER__STANDALONE_DATABASE_PATH",
        "PROMPT_FERRY_WORKER__UPSTREAM_BASE_URL",
        "PROMPT_FERRY_WORKER__UPSTREAM_API_KEY",
        "PROMPT_FERRY_WORKER__UPSTREAM_NATIVE_API",
        "PROMPT_FERRY_WORKER__WORKER_TOKEN",
        "PROMPT_FERRY_WORKER__MCP_WARMUP",
        "PROMPT_FERRY_WORKER__SHUTDOWN_DRAIN_SECONDS",
    ];
    let inherited = std::env::vars_os()
        .map(|(key, _)| key)
        .filter(|key| {
            key.to_str().is_some_and(|key| {
                key.starts_with("PROMPT_FERRY_") && !FIXTURE_ENV_KEYS.contains(&key)
            })
        })
        .collect::<Vec<_>>();
    for key in inherited {
        command.env_remove(key);
    }
}

async fn wait_for_public_addr(child: &mut Child, logs: &Arc<LogBuffer>) -> SocketAddr {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(addr) = logs
            .snapshot()
            .iter()
            .find_map(|line| parse_public_addr(line))
        {
            return addr;
        }
        if let Some(status) = child.try_wait().expect("poll child") {
            let logs = logs.snapshot().join("\n");
            panic!("prompt-ferry exited before listening ({status}); logs:\n{logs}");
        }
        if tokio::time::Instant::now() >= deadline {
            let logs = logs.snapshot().join("\n");
            panic!("prompt-ferry did not report its public address; logs:\n{logs}");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn parse_public_addr(line: &str) -> Option<SocketAddr> {
    let rest = line.split("public_addr=").nth(1)?;
    rest.split_whitespace().next()?.parse().ok()
}

fn unique_dir() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let base = std::env::var_os("PROMPT_FERRY_TEST_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let sequence = COUNTER.fetch_add(1, Ordering::SeqCst);
    base.join(format!(
        "prompt-ferry-graceful-{}-{sequence}",
        std::process::id()
    ))
}
