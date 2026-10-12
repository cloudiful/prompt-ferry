// Test-only PostgreSQL target guard.
//
// The database the integration tests may touch is never inferred from an
// application setting: it is named by `PROMPT_FERRY_QUOTA_TEST_DATABASE_URL`
// and authorized by `PROMPT_FERRY_QUOTA_TEST_CONFIRMED_TARGET`, the
// `host:port/database` identity an operator confirmed for this project. Both
// keys must be present and must agree before anything connects or migrates, so
// pointing the test URL at another server — including a production one — is
// refused instead of silently executed. `DATABASE_URL` is never read here.
//
// Every parse is pure and secret-safe: an error names the scheme, host, port,
// or database, never the URL, the username, or the password. Matching the
// confirmed identity is the authority; the URL only supplies the credentials.

use anyhow::{Result, bail};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};

/// The only environment key that names a dedicated test database.
pub const DATABASE_URL_KEY: &str = "PROMPT_FERRY_QUOTA_TEST_DATABASE_URL";

/// The operator-confirmed `host:port/database` this project may test against.
pub const CONFIRMED_TARGET_KEY: &str = "PROMPT_FERRY_QUOTA_TEST_CONFIRMED_TARGET";

const SUPPORTED_SCHEMES: [&str; 2] = ["postgres", "postgresql"];
const DEFAULT_PORT: u16 = 5432;

/// `HOST_CHARS` accepts a lowercase host label set; `DATABASE_CHARS` accepts an
/// unquoted PostgreSQL identifier. Both are deliberately narrow: a target this
/// guard cannot spell unambiguously is refused rather than guessed.
fn host_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-')
}

fn database_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '$')
}

/// A validated dedicated test target: an authorized identity plus the
/// credentials the dedicated URL carried for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestTarget {
    host: String,
    port: u16,
    database: String,
    username: String,
    password: Option<String>,
}

impl TestTarget {
    /// The `host:port/database` identity. It carries no userinfo, so it is the
    /// one form of this target that is safe to print.
    pub fn identity(&self) -> String {
        format!("{}:{}/{}", self.host, self.port, self.database)
    }

    /// Whether the dedicated URL carried a password, and it survived parsing
    /// into the connection options. A boolean rather than the credential: a
    /// guard case can pin that the password is forwarded without putting a
    /// secret-shaped string into test source or output.
    pub fn has_password(&self) -> bool {
        self.password.is_some()
    }

    /// Explicit credentials, with `.pgpass` and service files disabled: a test
    /// connection either uses the password the dedicated URL carried or fails.
    pub fn connect_options(&self) -> PgConnectOptions {
        let mut options = PgConnectOptions::new_without_pgpass()
            .host(&self.host)
            .port(self.port)
            .username(&self.username)
            .database(&self.database);
        if let Some(password) = &self.password {
            options = options.password(password);
        }
        options
    }
}

/// Validates the dedicated URL against the confirmed identity.
///
/// Pure, so the guard is exercised without a server and without any
/// environment: `identity` is the operator-confirmed `host:port/database`.
pub fn parse_target(url: &str, identity: &str) -> Result<TestTarget> {
    let confirmed = parse_identity(identity)?;
    let parsed = parse_url(url)?;

    if parsed.identity.label() != confirmed.label() {
        bail!(
            "{} addresses {} instead of the confirmed target {}; a test database must be \
             the one an operator confirmed",
            DATABASE_URL_KEY,
            parsed.identity.label(),
            confirmed.label()
        );
    }

    Ok(TestTarget {
        host: parsed.identity.host,
        port: parsed.identity.port,
        database: parsed.identity.database,
        username: parsed.username,
        password: parsed.password,
    })
}

/// Reads both dedicated keys and validates them against each other.
///
/// `Ok(None)` means neither key is configured, so a caller that owns an
/// early-return skip branch still has one. An optional caller may read that as
/// "no service"; it never licenses guessing a target. A configured URL or
/// identity that is malformed, incomplete, or mismatched is an error: the guard
/// fails closed instead of skipping.
pub fn resolve_target() -> Result<Option<TestTarget>> {
    let url = read_setting(DATABASE_URL_KEY);
    let identity = read_setting(CONFIRMED_TARGET_KEY);

    match (url, identity) {
        (None, None) => Ok(None),
        (Some(_), None) => bail!(
            "{CONFIRMED_TARGET_KEY} is required whenever {DATABASE_URL_KEY} is set; a test \
             database needs an operator-confirmed target"
        ),
        (None, Some(_)) => bail!(
            "{DATABASE_URL_KEY} is required whenever {CONFIRMED_TARGET_KEY} is set; a confirmed \
             target carries no credentials of its own"
        ),
        (Some(url), Some(identity)) => parse_target(&url, &identity).map(Some),
    }
}

/// A connected pool for the validated target, or `Ok(None)` when neither
/// dedicated key is configured.
pub async fn optional_pool(max_connections: u32) -> Result<Option<PgPool>> {
    let Some(target) = resolve_target()? else {
        return Ok(None);
    };
    connect(&target, max_connections).await.map(Some)
}

/// A connected pool for the validated target. Unlike [`optional_pool`] this
/// never reports "unconfigured": a caller that must exercise the database fails
/// when the dedicated keys are absent.
pub async fn required_pool(max_connections: u32) -> Result<PgPool> {
    let target = resolve_target()?.ok_or_else(|| {
        anyhow::anyhow!(
            "{DATABASE_URL_KEY} is required; run the tests through \
             scripts/ci/with-test-env.sh so the dedicated test settings are loaded"
        )
    })?;
    connect(&target, max_connections).await
}

async fn connect(target: &TestTarget, max_connections: u32) -> Result<PgPool> {
    Ok(PgPoolOptions::new()
        .max_connections(max_connections)
        .connect_with(target.connect_options())
        .await?)
}

fn read_setting(key: &str) -> Option<String> {
    let value = std::env::var(key).ok()?;
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// An endpoint identity: the normalized host, the effective port, and the
/// database. This is exactly what a confirmed target authorizes.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Identity {
    host: String,
    port: u16,
    database: String,
}

impl Identity {
    fn label(&self) -> String {
        format!("{}:{}/{}", self.host, self.port, self.database)
    }
}

/// A dedicated URL: the identity it addresses plus the credentials it carried.
/// Only the identity is ever compared or reported.
struct UrlParts {
    identity: Identity,
    username: String,
    password: Option<String>,
}

fn parse_url(url: &str) -> Result<UrlParts> {
    let (scheme, rest) = split_at(url, ':')
        .ok_or_else(|| anyhow::anyhow!("{DATABASE_URL_KEY} is not a valid PostgreSQL URL"))?;
    if !SUPPORTED_SCHEMES.contains(&scheme.to_ascii_lowercase().as_str()) {
        bail!("{DATABASE_URL_KEY} must use a postgres or postgresql scheme, not {scheme}");
    }
    let rest = rest
        .strip_prefix("//")
        .ok_or_else(|| anyhow::anyhow!("{DATABASE_URL_KEY} is not a valid PostgreSQL URL"))?;
    for (marker, what) in [('#', "a fragment"), ('?', "query parameters or options")] {
        if rest.contains(marker) {
            bail!("{DATABASE_URL_KEY} must not carry {what}");
        }
    }

    let (authority, database) = split_at(rest, '/')
        .ok_or_else(|| anyhow::anyhow!("{DATABASE_URL_KEY} must name a database explicitly"))?;
    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((user, host)) => (Some(user), host),
        None => (None, authority),
    };
    let Some(userinfo) = userinfo else {
        bail!("{DATABASE_URL_KEY} must carry a username");
    };
    let (username, password) = match userinfo.split_once(':') {
        Some((user, secret)) => (user, Some(decode(secret, DATABASE_URL_KEY)?)),
        None => (userinfo, None),
    };
    let username = decode(username, DATABASE_URL_KEY)?;
    if username.is_empty() {
        bail!("{DATABASE_URL_KEY} must carry a username");
    }

    let (host, port) = parse_host_port(hostport, DATABASE_URL_KEY)?;
    Ok(UrlParts {
        identity: Identity {
            host,
            port,
            database: parse_database(database, DATABASE_URL_KEY)?,
        },
        username,
        password,
    })
}

/// Parses the operator-confirmed `host:port/database` identity.
fn parse_identity(confirmed: &str) -> Result<Identity> {
    let (hostport, database) = split_at(confirmed, '/')
        .ok_or_else(|| anyhow::anyhow!("{CONFIRMED_TARGET_KEY} must read host:port/database"))?;
    let (host, port) = parse_host_port(hostport, CONFIRMED_TARGET_KEY)?;
    Ok(Identity {
        host,
        port,
        database: parse_database(database, CONFIRMED_TARGET_KEY)?,
    })
}

fn parse_database(value: &str, key: &str) -> Result<String> {
    if value.contains('/') {
        bail!("{key} must not carry a path beyond the database name");
    }
    let database = decode(value, key)?;
    check_charset(&database, "database name", key, database_char)
}

/// Splits `host[:port]`, reading an absent or empty port as 5432 so both sides
/// of a comparison agree on the default.
fn parse_host_port(value: &str, key: &str) -> Result<(String, u16)> {
    if value.starts_with('[') {
        bail!("{key} must use a named host; a bracketed IPv6 literal cannot be confirmed");
    }
    let (host, port) = split_at(value, ':').unwrap_or((value, ""));
    let port = if port.is_empty() {
        DEFAULT_PORT
    } else {
        port.parse::<u16>()
            .ok()
            .filter(|port| *port > 0)
            .ok_or_else(|| anyhow::anyhow!("{key} has an invalid port"))?
    };
    Ok((normalize_host(host, key)?, port))
}

fn normalize_host(value: &str, key: &str) -> Result<String> {
    let host = decode(value, key)?.to_ascii_lowercase();
    check_charset(&host, "host", key, host_char)
}

/// Decodes percent escapes without ever echoing the decoded text on failure.
fn decode(value: &str, key: &str) -> Result<String> {
    urlencoding::decode(value)
        .map(|decoded| decoded.into_owned())
        .map_err(|_| anyhow::anyhow!("{key} has a malformed percent escape"))
}

fn check_charset(value: &str, what: &str, key: &str, accepted: fn(char) -> bool) -> Result<String> {
    if value.is_empty() || !value.chars().all(accepted) {
        bail!("{key} has an unsupported {what}");
    }
    Ok(value.to_string())
}

/// Splits on the first occurrence, reporting `None` when it is absent.
fn split_at(value: &str, separator: char) -> Option<(&str, &str)> {
    value
        .find(separator)
        .map(|at| (&value[..at], &value[at + 1..]))
}
