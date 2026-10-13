//! Local diagnostic log (PRD §27.5): `<state>/logs/ixtable.log`, rotated at ~5 MB,
//! with secrets redacted before anything reaches disk (PRD §27.2).
//!
//! Every module may call [`log`]. Logging never fails the caller: I/O errors are dropped.
use crate::manager::AppError;
use chrono::Utc;
use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

pub const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
const LOG_FILE: &str = "ixtable.log";
/// Secret key words. A `key=value` / `"key": value` pair is masked when its key,
/// split into snake/kebab/camelCase words, contains one of these as a word run
/// (`access_token`, `refreshToken`, `X-Api-Key`, `client_secret`, `DEK`).
/// Plurals are listed explicitly so innocent words (`desk`, `index`) stay clear.
const SECRET_KEYS: [&str; 23] = [
    "password",
    "passwords",
    "passwd",
    "pwd",
    "secret",
    "secrets",
    "token",
    "tokens",
    "api_key",
    "apikey",
    "authorization",
    "passphrase",
    "dek",
    "private_key",
    "privatekey",
    "credential",
    "credentials",
    "accesstoken",
    "refreshtoken",
    "authtoken",
    "clientsecret",
    "secretkey",
    "cookie",
];
/// Words that also name counts (`max_tokens=100`): a purely numeric value stays clear.
const COUNT_KEYS: [&str; 2] = ["token", "tokens"];
/// HTTP auth schemes: in `Bearer abc` the credential after the scheme is masked too.
const AUTH_SCHEMES: [&str; 8] = [
    "bearer",
    "basic",
    "digest",
    "token",
    "negotiate",
    "ntlm",
    "apikey",
    "hoba",
];
const MASK: &str = "***";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub area: String,
    pub message: String,
}

/// A size-capped log file with one rotated generation (`<file>.1`).
pub struct Logger {
    path: PathBuf,
    max_bytes: u64,
}
impl Logger {
    pub fn new(path: PathBuf, max_bytes: u64) -> Self {
        Self { path, max_bytes }
    }
    fn rotated(&self) -> PathBuf {
        let mut name = self.path.as_os_str().to_owned();
        name.push(".1");
        PathBuf::from(name)
    }
    pub fn write(&self, level: &str, area: &str, message: &str) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let line = format!(
            "{}\t{}\t{}\t{}\n",
            Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            one_line(&level.to_ascii_uppercase()),
            one_line(area),
            one_line(&redact(message))
        );
        let size = fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        if size > 0 && size + line.len() as u64 > self.max_bytes {
            fs::rename(&self.path, self.rotated())?;
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?
            .write_all(line.as_bytes())
    }
    /// The newest `limit` entries, oldest first, spanning the rotated file when needed.
    pub fn read(&self, limit: usize) -> Vec<LogEntry> {
        let mut lines: Vec<String> = Vec::new();
        for file in [self.rotated(), self.path.clone()] {
            if let Ok(text) = fs::read_to_string(&file) {
                lines.extend(text.lines().map(str::to_owned));
            }
        }
        let start = lines.len().saturating_sub(limit);
        lines[start..].iter().filter_map(|l| parse(l)).collect()
    }
}

fn parse(line: &str) -> Option<LogEntry> {
    let mut parts = line.splitn(4, '\t');
    Some(LogEntry {
        timestamp: parts.next()?.into(),
        level: parts.next()?.into(),
        area: parts.next()?.into(),
        message: parts.next()?.into(),
    })
}
fn one_line(text: &str) -> String {
    text.replace(['\n', '\r', '\t'], " ")
}

static DIR: OnceLock<PathBuf> = OnceLock::new();
static LOCK: Mutex<()> = Mutex::new(());

/// Points the global log at `<dir>/ixtable.log`. The first call wins.
pub fn init(dir: PathBuf) {
    let _ = DIR.set(dir);
}
pub fn log_dir() -> PathBuf {
    DIR.get_or_init(|| crate::paths::state_dir().join("logs"))
        .clone()
}
fn global() -> Logger {
    Logger::new(log_dir().join(LOG_FILE), MAX_LOG_BYTES)
}

/// Appends one redacted line to the local diagnostic log. Never fails.
pub fn log(level: &str, area: &str, message: &str) {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = global().write(level, area, message);
}
pub fn info(area: &str, message: &str) {
    log("info", area, message);
}
pub fn warn(area: &str, message: &str) {
    log("warn", area, message);
}
pub fn error(area: &str, message: &str) {
    log("error", area, message);
}

/// One local crash record for a Rust panic (PRD §27.5; never uploaded). The
/// message is redacted by [`log`] like every other line.
pub fn panic_record(thread: Option<&str>, payload: &str, location: Option<String>) -> String {
    let one_line = payload.replace(['\n', '\r'], " ");
    format!(
        "panic in thread {:?} at {}: {one_line}",
        thread.unwrap_or("<unnamed>"),
        location.as_deref().unwrap_or("<unknown>")
    )
}

const PANIC_LOCK_WAIT: std::time::Duration = std::time::Duration::from_millis(200);

/// Retries `try_lock` on the log lock until `wait` elapses (poisoning is ignored).
fn lock_within(wait: std::time::Duration) -> Option<std::sync::MutexGuard<'static, ()>> {
    let deadline = std::time::Instant::now() + wait;
    loop {
        match LOCK.try_lock() {
            Ok(guard) => return Some(guard),
            Err(std::sync::TryLockError::Poisoned(e)) => return Some(e.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) => {}
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// The line written to stderr when the panic record cannot reach the log file.
fn stderr_fallback(record: &str) -> String {
    format!("ixtable log (lock busy): ERROR panic {}", redact(record))
}

/// Chains a panic hook that appends a redacted `panic` record to the local log,
/// then runs the previous hook (stderr output and backtrace stay as they were).
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<non-string panic payload>".into());
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
        let thread = std::thread::current();
        let record = panic_record(thread.name(), &payload, location);
        // A panic while this thread holds the log lock must not deadlock the hook:
        // wait briefly for another thread's write, then fall back to stderr.
        match lock_within(PANIC_LOCK_WAIT) {
            Some(_guard) => {
                let _ = global().write("error", "panic", &record);
            }
            None => eprintln!("{}", stderr_fallback(&record)),
        }
        previous(info);
    }));
}

/// Masks URL credentials (`scheme://user:pass@`) and `key=value` / `"key": "value"` secrets.
pub fn redact(text: &str) -> String {
    redact_keys(&redact_urls(text))
}

fn redact_urls(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("://") {
        let (head, tail) = rest.split_at(i + 3);
        out.push_str(head);
        // Passwords may contain `@ / ? #`: take the whole token and split
        // user info at its LAST `@` (over-masking beats leaking).
        let end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>'))
            .unwrap_or(tail.len());
        let authority = &tail[..end];
        match authority
            .rfind('@')
            .and_then(|at| authority[..at].find(':').map(|colon| (at, colon)))
        {
            Some((at, colon)) => {
                out.push_str(&authority[..=colon]);
                out.push_str(MASK);
                out.push_str(&authority[at..]);
            }
            None => out.push_str(authority),
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// The [`SECRET_KEYS`] entries an identifier contains as a word run (words split
/// at `_`, `-`, and lower-to-upper case changes).
fn secret_words(key: &str) -> Vec<&'static str> {
    let mut words = String::with_capacity(key.len() + 4);
    let mut prev_lower = false;
    for c in key.chars() {
        if c == '_' || c == '-' {
            words.push('_');
            prev_lower = false;
            continue;
        }
        if c.is_ascii_uppercase() && prev_lower {
            words.push('_');
        }
        prev_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
        words.push(c.to_ascii_lowercase());
    }
    let words = format!("_{words}_");
    SECRET_KEYS
        .iter()
        .copied()
        .filter(|k| words.contains(&format!("_{k}_")))
        .collect()
}

fn redact_keys(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if !is_key_char(bytes[i]) || (i > 0 && is_key_char(bytes[i - 1])) {
            let ch = text[i..].chars().next().unwrap_or(' ');
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        let key_end = (i..bytes.len())
            .find(|&k| !is_key_char(bytes[k]))
            .unwrap_or(bytes.len());
        let found = secret_words(&text[i..key_end]);
        let span = (!found.is_empty())
            .then(|| secret_value(bytes, key_end, found.contains(&"authorization")))
            .flatten()
            .filter(|&(s, e)| {
                !(found.iter().all(|w| COUNT_KEYS.contains(w))
                    && bytes[s..e].iter().all(u8::is_ascii_digit))
            });
        match span {
            Some((value_start, value_end)) => {
                out.push_str(&text[i..value_start]);
                out.push_str(MASK);
                i = value_end;
            }
            None => {
                out.push_str(&text[i..key_end]);
                i = key_end;
            }
        }
    }
    out
}
/// The value span after a key ending at `j`: an optional closing quote, spaces,
/// `=` or `:`, spaces, then a quoted or bare value. A bare value that is an auth
/// scheme (any word when `auth_header`) runs on through the credential after it
/// (`Bearer abc.def`). `None` when no value follows.
fn secret_value(bytes: &[u8], mut j: usize, auth_header: bool) -> Option<(usize, usize)> {
    if j < bytes.len() && matches!(bytes[j], b'"' | b'\'') {
        j += 1;
    }
    while j < bytes.len() && bytes[j] == b' ' {
        j += 1;
    }
    if j >= bytes.len() || !matches!(bytes[j], b'=' | b':') {
        return None;
    }
    j += 1;
    while j < bytes.len() && bytes[j] == b' ' {
        j += 1;
    }
    let (start, end) = if j < bytes.len() && matches!(bytes[j], b'"' | b'\'') {
        (j + 1, closing_quote(bytes, j))
    } else {
        let end = bare_end(bytes, j);
        let word = &bytes[j..end];
        let is_scheme = word.iter().all(u8::is_ascii_alphabetic)
            && (auth_header
                || AUTH_SCHEMES
                    .iter()
                    .any(|s| s.as_bytes().eq_ignore_ascii_case(word)));
        let next = end + 1;
        if is_scheme && bytes.get(end) == Some(&b' ') && bare_end(bytes, next) > next {
            (j, bare_end(bytes, next))
        } else {
            (j, end)
        }
    };
    (end > start).then_some((start, end))
}
/// End of a bare (unquoted) value starting at `j`.
fn bare_end(bytes: &[u8], j: usize) -> usize {
    (j..bytes.len())
        .find(|&k| {
            bytes[k].is_ascii_whitespace()
                || matches!(bytes[k], b'"' | b'\'' | b';' | b'&' | b',' | b'}')
        })
        .unwrap_or(bytes.len())
}
/// Index of the quote closing the value opened at `open` (or the end of the
/// text): honors backslash escapes (`'it\\'s'`) and doubled quotes (`'it''s'`).
fn closing_quote(bytes: &[u8], open: usize) -> usize {
    let quote = bytes[open];
    let mut k = open + 1;
    while k < bytes.len() {
        match bytes[k] {
            b'\\' => k += 2,
            b if b == quote && bytes.get(k + 1) == Some(&quote) => k += 2,
            b if b == quote => return k,
            _ => k += 1,
        }
    }
    bytes.len()
}
fn is_key_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

/// The newest `limit` (default 200, max 5000) diagnostic log entries.
pub fn read_logs(window_label: String, limit: Option<usize>) -> Result<Vec<LogEntry>, AppError> {
    let _ = window_label;
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    Ok(global().read(limit.unwrap_or(200).min(5000)))
}

/// Writes a frontend diagnostic line (level `info`/`warn`/`error`) to the local log.
pub fn write_log(
    window_label: String,
    level: String,
    area: String,
    message: String,
) -> Result<(), AppError> {
    let _ = window_label;
    let level = match level.as_str() {
        "warn" | "error" | "debug" => level,
        _ => "info".into(),
    };
    log(&level, &format!("ui:{area}"), &message);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_connection_strings_and_password_pairs() {
        let text = "connect postgres://admin:s3cr3t@db.local:5432/app failed";
        assert_eq!(
            redact(text),
            "connect postgres://admin:***@db.local:5432/app failed"
        );
        assert_eq!(
            redact("host=db user=me password=hunter2 sslmode=require"),
            "host=db user=me password=*** sslmode=require"
        );
        assert_eq!(
            redact(r#"{"password": "hunter2", "name": "x"}"#),
            r#"{"password": "***", "name": "x"}"#
        );
        assert_eq!(redact("Server=x;Pwd=abc;Db=y"), "Server=x;Pwd=***;Db=y");
        assert_eq!(redact("API_KEY: sk-123"), "API_KEY: ***");
        assert_eq!(redact("https://example.com/a"), "https://example.com/a");
        assert_eq!(redact("passwords are fine"), "passwords are fine");
        assert_eq!(redact("über token=x ✓"), "über token=*** ✓");
    }

    #[test]
    fn panic_records_are_one_redacted_line() {
        let record = panic_record(
            Some("main"),
            "connect failed\npassword=hunter2",
            Some("src/x.rs:1:2".into()),
        );
        assert_eq!(
            record,
            "panic in thread \"main\" at src/x.rs:1:2: connect failed password=hunter2"
        );
        assert_eq!(redact(&record), record.replace("hunter2", MASK));
        assert!(panic_record(None, "boom", None).contains("<unnamed>"));
    }

    #[test]
    fn redacts_secret_keys_in_any_naming_style() {
        for (input, expected) in [
            ("access_token=abc123 next", "access_token=*** next"),
            ("refresh_token: abc", "refresh_token: ***"),
            (
                r#"{"refresh_token": "r-1", "n": 1}"#,
                r#"{"refresh_token": "***", "n": 1}"#,
            ),
            (r#"{"accessToken":"a.b.c"}"#, r#"{"accessToken":"***"}"#),
            ("clientSecret=s", "clientSecret=***"),
            ("X-Api-Key: k1", "X-Api-Key: ***"),
            ("APIKey=k2", "APIKey=***"),
            (
                "GET /cb?code=1&access_token=t0k&state=2",
                "GET /cb?code=1&access_token=***&state=2",
            ),
            ("dek=AAAA", "dek=***"),
            (r#"{"wrappedDek": "QUJD"}"#, r#"{"wrappedDek": "***"}"#),
            ("db_password='p w'", "db_password='***'"),
            ("private_key=-----BEGIN", "private_key=***"),
        ] {
            assert_eq!(redact(input), expected, "{input}");
        }
        for safe in [
            "max_tokens=100",
            "index=3",
            "desk=1",
            "tokens: 5",
            "the token was refreshed",
            "passwords are fine",
        ] {
            assert_eq!(redact(safe), safe);
        }
    }

    #[test]
    fn redacts_quoted_values_with_spaces_and_escapes() {
        for (input, expected) in [
            ("password='ab cd' user=x", "password='***' user=x"),
            (r"password='it\'s' user=x", "password='***' user=x"),
            ("password='it''s secret' user=x", "password='***' user=x"),
            (r#"pwd="x y" db=z"#, r#"pwd="***" db=z"#),
            (
                r#"{"password": "a \"b\" c", "n": 1}"#,
                r#"{"password": "***", "n": 1}"#,
            ),
            ("password='unterminated secret", "password='***"),
            ("password='' user=x", "password='' user=x"),
        ] {
            assert_eq!(redact(input), expected, "{input}");
        }
        for (input, expected) in [
            (
                "postgres://admin:p@ss%20w@rd@db.local/app",
                "postgres://admin:***@db.local/app",
            ),
            (
                "postgres://u:pa/ss?#x@db.local/app",
                "postgres://u:***@db.local/app",
            ),
            ("'postgres://u:p@ss@h/db'", "'postgres://u:***@h/db'"),
        ] {
            assert_eq!(redact(input), expected, "{input}");
        }
        assert_eq!(
            redact("postgres://admin:p@ss@db.local:5432/app x"),
            "postgres://admin:***@db.local:5432/app x"
        );
        assert_eq!(
            redact("see https://example.com/users/@me"),
            "see https://example.com/users/@me"
        );
    }

    #[test]
    fn redacts_plural_and_compound_secret_keys() {
        for (input, expected) in [
            ("passwords=a,b", "passwords=***,b"),
            ("secrets=s1 x", "secrets=*** x"),
            ("tokens=abc x", "tokens=*** x"),
            ("credential=c1", "credential=***"),
            ("credentials: c2", "credentials: ***"),
            ("passwd=p", "passwd=***"),
            ("apikey=k", "apikey=***"),
            ("clientsecret=s", "clientsecret=***"),
            ("privateKey=k", "privateKey=***"),
            ("pwd=1234", "pwd=***"),
        ] {
            assert_eq!(redact(input), expected, "{input}");
        }
        for safe in [
            "max_tokens=100",
            "tokens: 5",
            "desk=1",
            "index=3",
            "passport=1",
            "secretary=bob",
            "tokenizer=bpe",
            "compass=north",
        ] {
            assert_eq!(redact(safe), safe);
        }
    }

    #[test]
    fn redacts_auth_scheme_and_credential_as_one_value() {
        for (input, expected) in [
            (
                "Authorization: Bearer abc.def next",
                "Authorization: *** next",
            ),
            ("authorization=Basic dXNlcjpw x", "authorization=*** x"),
            ("Authorization: Custom k1 x", "Authorization: *** x"),
            ("token: Bearer abc x", "token: *** x"),
            (
                r#"{"Authorization": "Bearer a b"}"#,
                r#"{"Authorization": "***"}"#,
            ),
            ("Authorization: Bearer", "Authorization: ***"),
        ] {
            assert_eq!(redact(input), expected, "{input}");
        }
        assert_eq!(redact("password=abc user=x"), "password=*** user=x");
    }

    #[test]
    fn panic_lock_wait_falls_back_when_busy() {
        let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let started = std::time::Instant::now();
        let waited =
            std::thread::spawn(|| lock_within(std::time::Duration::from_millis(50)).is_some())
                .join()
                .unwrap();
        assert!(!waited);
        assert!(started.elapsed() >= std::time::Duration::from_millis(50));
        drop(guard);
        let holder = std::thread::spawn(|| {
            let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
            std::thread::sleep(std::time::Duration::from_millis(30));
        });
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert!(lock_within(PANIC_LOCK_WAIT).is_some());
        holder.join().unwrap();
        assert_eq!(
            stderr_fallback("boom password=pw"),
            "ixtable log (lock busy): ERROR panic boom password=***"
        );
    }

    #[test]
    fn rotates_and_reads_across_generations() {
        let dir = std::env::temp_dir().join(format!("ixtable-log-{}", uuid::Uuid::new_v4()));
        let logger = Logger::new(dir.join("test.log"), 300);
        for n in 0..20 {
            logger
                .write("info", "save", &format!("event {n} password=pw{n}\nnext"))
                .unwrap();
        }
        assert!(fs::metadata(dir.join("test.log")).unwrap().len() <= 300);
        assert!(dir.join("test.log.1").exists());
        let entries = logger.read(3);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[2].message, "event 19 password=*** next");
        assert_eq!(entries[2].level, "INFO");
        assert_eq!(entries[2].area, "save");
        fs::remove_dir_all(dir).unwrap();
    }
}
