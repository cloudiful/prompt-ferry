//! Repository `.env` loading for configuration.
//!
//! The file is read as `KEY=VALUE` **data**: values are never expanded,
//! evaluated, or executed, and a key already present in the process
//! environment is never replaced. This keeps the precedence an operator
//! already relies on (explicit export beats file, file beats built-in
//! default) and lets `cargo run` pick up local values without a shell
//! wrapper, while a `$` in a password stays a literal `$` instead of
//! triggering shell semantics.
//!
//! Nothing loaded here is ever returned, logged, or echoed: the caller only
//! learns *which* file was read.

use anyhow::Context;
use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
};

/// Environment variable naming an explicit dotenv file.
pub const DOTENV_PATH_ENV: &str = "PROMPT_FERRY_DOTENV";

const DOTENV_FILE_NAME: &str = ".env";

/// A dotenv file that was found, and whether the operator asked for it by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DotenvSource {
    pub path: PathBuf,
    /// True when [`DOTENV_PATH_ENV`] selected the file, so a missing or
    /// unreadable path is an operator mistake worth reporting.
    pub explicit: bool,
}

/// Read the repository `.env` before the configuration layer runs.
///
/// Returns the file that was loaded, or `None` when no dotenv file applies.
/// An explicitly configured but unusable file is an error; a discovered file
/// that simply is not there is not.
pub fn load_repository_env() -> anyhow::Result<Option<PathBuf>> {
    let Some(source) = resolve_repository_env() else {
        return Ok(None);
    };
    if !source.path.is_file() {
        if source.explicit {
            anyhow::bail!(
                "`{DOTENV_PATH_ENV}` points at `{}`, which is not a readable file",
                source.path.display()
            );
        }
        return Ok(None);
    }
    load_env_data(&source.path)
        .with_context(|| format!("failed to load `{}` as dotenv data", source.path.display()))?;
    Ok(Some(source.path))
}

/// Locate the dotenv file to read, without reading it.
///
/// Resolution order: [`DOTENV_PATH_ENV`], then `.env` beside the working
/// directory, then `.env` beside the executable (the packaged desktop
/// layout).
fn resolve_repository_env() -> Option<DotenvSource> {
    if let Some(path) = env_path(DOTENV_PATH_ENV) {
        return Some(DotenvSource {
            path,
            explicit: true,
        });
    }
    if let Ok(working_dir) = env::current_dir() {
        let path = working_dir.join(DOTENV_FILE_NAME);
        if path.is_file() {
            return Some(DotenvSource {
                path,
                explicit: false,
            });
        }
    }
    let path = env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))?
        .join(DOTENV_FILE_NAME);
    path.is_file().then_some(DotenvSource {
        path,
        explicit: false,
    })
}

fn env_path(key: &str) -> Option<PathBuf> {
    let value = env::var_os(key).filter(|value| !value.is_empty())?;
    Some(PathBuf::from(value))
}

/// Parse `path` as dotenv data and publish every key the process environment
/// does not already define.
fn load_env_data(path: &Path) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read `{}`", path.display()))?;
    for line in content.lines() {
        let Some((key, value)) = parse_line(line) else {
            continue;
        };
        if env::var_os(&key).is_none() {
            // Safety: startup is single-threaded before any worker task runs.
            unsafe { env::set_var(key, value) };
        }
    }
    Ok(())
}

/// Split one `KEY=VALUE` line, or return `None` for a blank line, a comment,
/// or a line that is not an assignment.
///
/// Values keep every character verbatim apart from surrounding whitespace and
/// one layer of matching quotes; a `$` or a backtick stays part of the value
/// because this reader never evaluates anything.
fn parse_line(line: &str) -> Option<(String, OsString)> {
    let line = line.trim().strip_prefix('\u{feff}').unwrap_or(line).trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    if !is_env_key(key) {
        return None;
    }
    let mut value = value.trim();
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        value = &value[1..value.len() - 1];
    }
    Some((key.to_string(), OsString::from(value)))
}

fn is_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_alphabetic() || first == '_')
        && chars.all(|char| char.is_ascii_alphanumeric() || char == '_')
}

#[cfg(test)]
mod tests {
    use super::{DOTENV_PATH_ENV, is_env_key, load_env_data, parse_line, resolve_repository_env};
    use std::{
        ffi::OsString,
        path::PathBuf,
        sync::{Mutex, MutexGuard, OnceLock},
    };

    fn unique_key(label: &str) -> String {
        format!(
            "PROMPT_FERRY_DOTENV_TEST_{label}_{}",
            uuid::Uuid::new_v4().simple()
        )
    }

    /// Environment mutation is process-wide, so dotenv tests run one at a time.
    fn env_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn dotenv_file(name: &str, contents: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "prompt-ferry-dotenv-{name}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::write(&path, contents).expect("write dotenv fixture");
        path
    }

    #[test]
    fn parses_assignments_and_skips_comments_and_blanks() {
        assert_eq!(
            parse_line("  KEY = value  "),
            Some(("KEY".to_string(), OsString::from("value")))
        );
        assert_eq!(
            parse_line("QUOTED=\"  spaced value \""),
            Some(("QUOTED".to_string(), OsString::from("  spaced value ")))
        );
        assert_eq!(
            parse_line("SINGLE='raw'"),
            Some(("SINGLE".to_string(), OsString::from("raw")))
        );
        assert_eq!(parse_line("# a comment"), None);
        assert_eq!(parse_line("   "), None);
        assert_eq!(parse_line("no assignment here"), None);
        assert_eq!(parse_line("1BAD=value"), None);
        assert_eq!(parse_line("BAD-KEY=value"), None);
    }

    #[test]
    fn keeps_shell_syntax_literal_because_nothing_is_evaluated() {
        assert_eq!(
            parse_line("PASSWORD=$(id) `whoami` $HOME ${HOME}"),
            Some((
                "PASSWORD".to_string(),
                OsString::from("$(id) `whoami` $HOME ${HOME}")
            ))
        );
    }

    #[test]
    fn recognises_only_shell_compatible_env_keys() {
        assert!(is_env_key("_A1"));
        assert!(is_env_key("PROMPT_FERRY_RELAY__BIND"));
        assert!(!is_env_key("1A"));
        assert!(!is_env_key("A-B"));
        assert!(!is_env_key(""));
    }

    #[test]
    fn loads_values_without_overriding_the_process_environment() {
        let _guard = env_lock();
        let from_file = unique_key("FILE");
        let from_env = unique_key("ENV");
        let path = dotenv_file(
            "precedence",
            &format!(
                "{from_file}=from-file\n{from_env}=from-file\n# comment\n\nnot-an-assignment\n"
            ),
        );
        unsafe { std::env::set_var(&from_env, "from-environment") };

        load_env_data(&path).expect("dotenv loads");

        assert_eq!(
            std::env::var(&from_file).ok(),
            Some("from-file".to_string())
        );
        assert_eq!(
            std::env::var(&from_env).ok(),
            Some("from-environment".to_string()),
            "an explicit process value must keep precedence over the file"
        );

        unsafe {
            std::env::remove_var(&from_file);
            std::env::remove_var(&from_env);
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn loads_a_password_containing_a_dollar_sign_verbatim() {
        let _guard = env_lock();
        let key = unique_key("DOLLAR");
        let path = dotenv_file("dollar", &format!("{key}=p@ss$word\n"));

        load_env_data(&path).expect("dotenv loads");

        assert_eq!(std::env::var(&key).ok(), Some("p@ss$word".to_string()));

        unsafe { std::env::remove_var(&key) };
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn reports_a_missing_file_instead_of_failing() {
        let _guard = env_lock();
        let error = load_env_data(std::path::Path::new("/nonexistent/prompt-ferry.env"))
            .expect_err("a missing file cannot be read");
        assert!(error.to_string().contains("prompt-ferry.env"));
    }

    #[test]
    fn resolves_an_explicit_path_even_when_it_does_not_exist() {
        let _guard = env_lock();
        let explicit = unique_key("PATH");
        unsafe { std::env::set_var(DOTENV_PATH_ENV, "/nonexistent/explicit.env") };

        let source = resolve_repository_env().expect("explicit path always resolves");
        assert!(source.explicit);
        assert_eq!(source.path, PathBuf::from("/nonexistent/explicit.env"));

        unsafe { std::env::remove_var(DOTENV_PATH_ENV) };
        let _ = explicit;
    }
}
