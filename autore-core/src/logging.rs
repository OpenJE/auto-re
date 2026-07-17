//! Logging infrastructure with secret redaction and panic-aware terminal restoration.
//!
//! Provides `init_logging()` that configures `tracing` with:
//! - File or stderr output (never stdout to avoid corrupting JSON CLI output)
//! - JSON or plain text format
//! - Automatic redaction of fields containing sensitive keywords
//! - Panic hook that restores terminal state before displaying panic info

use std::fmt;
use std::fs::File;
use std::io;
use std::panic;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use tracing::Event;
use tracing::field::{Field, Visit};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, Registry};

use crate::Error;

/// Sensitive field name patterns that should be redacted in logs.
const SENSITIVE_PATTERNS: &[&str] = &["key", "token", "secret", "password", "credential"];

/// Type alias for the terminal restore hook used in panic recovery.
type TerminalRestoreHook = Arc<Mutex<Option<Box<dyn Fn() + Send + Sync>>>>;

/// Global terminal restoration hook for panic recovery.
static TERMINAL_RESTORE_HOOK: OnceLock<TerminalRestoreHook> = OnceLock::new();

fn get_terminal_restore_hook() -> &'static TerminalRestoreHook {
    TERMINAL_RESTORE_HOOK.get_or_init(|| Arc::new(Mutex::new(None)))
}

/// Registers a callback to restore terminal state on panic.
///
/// Call this before entering TUI mode. The callback will be invoked if a panic
/// occurs, allowing the terminal to be restored before the panic is displayed.
pub fn set_terminal_restore_hook(hook: Box<dyn Fn() + Send + Sync>) {
    let lock = get_terminal_restore_hook();
    *lock.lock().unwrap() = Some(hook);
}

/// Installs a panic hook that restores terminal state before displaying the panic.
///
/// This preserves the original panic information while ensuring the terminal
/// is returned to a usable state.
pub fn install_panic_hook() {
    let original_hook = panic::take_hook();
    let restore_hook = Arc::clone(get_terminal_restore_hook());

    panic::set_hook(Box::new(move |info| {
        if let Ok(guard) = restore_hook.lock()
            && let Some(ref restore_fn) = *guard
        {
            restore_fn();
        }
        original_hook(info);
    }));
}

/// Configuration for the logging subsystem.
#[derive(Debug, Clone, Default)]
pub struct LoggingConfig {
    /// Path to log file. If `None`, logs to stderr.
    /// In TUI mode, this MUST be `Some` to avoid corrupting the display.
    pub log_file: Option<PathBuf>,
    /// If `true`, emit logs in JSON format. Otherwise, use plain text.
    pub json: bool,
}

// ---------------------------------------------------------------------------
// Redaction

/// Checks if a field name contains a sensitive pattern.
pub fn is_sensitive_field(name: &str) -> bool {
    let lower = name.to_lowercase();
    SENSITIVE_PATTERNS
        .iter()
        .any(|pattern| lower.contains(pattern))
}

/// Redacts a value if the field name is sensitive.
pub fn redact_if_sensitive(name: &str, value: &str) -> String {
    if is_sensitive_field(name) {
        "<redacted>".to_string()
    } else {
        value.to_string()
    }
}

/// A field visitor that collects fields with sensitive values redacted.
struct RedactingFieldVisitor {
    fields: Vec<(String, String)>,
}

impl Visit for RedactingFieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let name = field.name();
        let val = format!("{:?}", value);
        self.fields
            .push((name.to_string(), redact_if_sensitive(name, &val)));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        let name = field.name();
        self.fields
            .push((name.to_string(), redact_if_sensitive(name, value)));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        let name = field.name();
        self.fields.push((
            name.to_string(),
            redact_if_sensitive(name, &value.to_string()),
        ));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        let name = field.name();
        self.fields.push((
            name.to_string(),
            redact_if_sensitive(name, &value.to_string()),
        ));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        let name = field.name();
        self.fields.push((
            name.to_string(),
            redact_if_sensitive(name, &value.to_string()),
        ));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        let name = field.name();
        self.fields.push((
            name.to_string(),
            redact_if_sensitive(name, &value.to_string()),
        ));
    }
}

/// Custom event formatter that redacts sensitive fields in plain-text output.
struct RedactingFormatter;

impl<S, N> FormatEvent<S, N> for RedactingFormatter
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let metadata = event.metadata();
        let level = metadata.level();
        let target = metadata.target();

        let mut visitor = RedactingFieldVisitor { fields: Vec::new() };
        event.record(&mut visitor);

        write!(writer, "{} {}: ", level, target)?;

        let mut first = true;
        for (name, value) in &visitor.fields {
            if !first {
                write!(writer, " ")?;
            }
            if name == "message" {
                write!(writer, "{}", value)?;
            } else {
                write!(writer, "{}={}", name, value)?;
            }
            first = false;
        }
        writeln!(writer)
    }
}

/// Custom JSON event formatter that redacts sensitive fields.
struct RedactingJsonFormatter;

impl<S, N> FormatEvent<S, N> for RedactingJsonFormatter
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let metadata = event.metadata();

        let mut visitor = RedactingFieldVisitor { fields: Vec::new() };
        event.record(&mut visitor);

        // Build JSON object manually with redacted fields
        write!(writer, "{{\"level\":\"{}\"", metadata.level())?;
        write!(writer, ",\"target\":\"{}\"", metadata.target())?;

        for (name, value) in &visitor.fields {
            // Escape any quotes in values
            let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
            write!(writer, ",\"{}\":\"{}\"", name, escaped)?;
        }

        writeln!(writer, "}}")
    }
}

// ---------------------------------------------------------------------------
// Initialization

/// Initializes the logging subsystem with the given configuration.
///
/// # Errors
///
/// Returns an error if:
/// - The log file cannot be opened
/// - The logging system has already been initialized
pub fn init_logging(config: LoggingConfig) -> crate::Result<()> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    match config.log_file {
        Some(path) => {
            let file = File::options()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|e| {
                    Error::Io(io::Error::new(
                        e.kind(),
                        format!("failed to open log file {:?}: {}", path, e),
                    ))
                })?;

            if config.json {
                let layer = tracing_subscriber::fmt::layer()
                    .event_format(RedactingJsonFormatter)
                    .with_writer(Mutex::new(file))
                    .with_filter(env_filter);

                Registry::default()
                    .with(layer)
                    .try_init()
                    .map_err(|e| Error::Operation(format!("logging already initialized: {}", e)))?;
            } else {
                let layer = tracing_subscriber::fmt::layer()
                    .event_format(RedactingFormatter)
                    .with_writer(Mutex::new(file))
                    .with_filter(env_filter);

                Registry::default()
                    .with(layer)
                    .try_init()
                    .map_err(|e| Error::Operation(format!("logging already initialized: {}", e)))?;
            }
        }
        None => {
            if config.json {
                let layer = tracing_subscriber::fmt::layer()
                    .event_format(RedactingJsonFormatter)
                    .with_writer(io::stderr)
                    .with_filter(env_filter);

                Registry::default()
                    .with(layer)
                    .try_init()
                    .map_err(|e| Error::Operation(format!("logging already initialized: {}", e)))?;
            } else {
                let layer = tracing_subscriber::fmt::layer()
                    .event_format(RedactingFormatter)
                    .with_writer(io::stderr)
                    .with_filter(env_filter);

                Registry::default()
                    .with(layer)
                    .try_init()
                    .map_err(|e| Error::Operation(format!("logging already initialized: {}", e)))?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tempfile::TempDir;

    #[test]
    fn logging_config_default() {
        let config = LoggingConfig::default();
        assert!(config.log_file.is_none());
        assert!(!config.json);
    }

    #[test]
    fn logging_config_custom() {
        let config = LoggingConfig {
            log_file: Some(PathBuf::from("test.log")),
            json: true,
        };
        assert_eq!(config.log_file, Some(PathBuf::from("test.log")));
        assert!(config.json);
    }

    // -----------------------------------------------------------------------
    // Redaction tests

    #[test]
    fn is_sensitive_field_detects_key() {
        assert!(is_sensitive_field("api_key"));
        assert!(is_sensitive_field("API_KEY"));
        assert!(is_sensitive_field("my_key_field"));
    }

    #[test]
    fn is_sensitive_field_detects_token() {
        assert!(is_sensitive_field("access_token"));
        assert!(is_sensitive_field("TOKEN"));
    }

    #[test]
    fn is_sensitive_field_detects_secret() {
        assert!(is_sensitive_field("client_secret"));
        assert!(is_sensitive_field("SECRET_VALUE"));
    }

    #[test]
    fn is_sensitive_field_detects_password() {
        assert!(is_sensitive_field("user_password"));
        assert!(is_sensitive_field("PASSWORD"));
    }

    #[test]
    fn is_sensitive_field_detects_credential() {
        assert!(is_sensitive_field("aws_credential"));
        assert!(is_sensitive_field("CREDENTIAL"));
    }

    #[test]
    fn is_sensitive_field_rejects_safe_names() {
        assert!(!is_sensitive_field("user_id"));
        assert!(!is_sensitive_field("timestamp"));
        assert!(!is_sensitive_field("count"));
    }

    #[test]
    fn redact_if_sensitive_redacts() {
        assert_eq!(redact_if_sensitive("api_key", "secret123"), "<redacted>");
        assert_eq!(redact_if_sensitive("password", "p@ss"), "<redacted>");
    }

    #[test]
    fn redact_if_sensitive_preserves_safe() {
        assert_eq!(redact_if_sensitive("user_id", "12345"), "12345");
    }

    #[test]
    fn no_secret_in_logs_redacts_password_field() {
        let password_value = "super_secret_password_123";
        let redacted = redact_if_sensitive("password", password_value);
        assert_eq!(redacted, "<redacted>");
        assert!(!redacted.contains(password_value));
    }

    #[test]
    fn no_secret_in_logs_redacts_api_key_field() {
        let api_key_value = "sk_test_1234567890abcdef";
        let redacted = redact_if_sensitive("api_key", api_key_value);
        assert_eq!(redacted, "<redacted>");
        assert!(!redacted.contains(api_key_value));
    }

    #[test]
    fn logging_json_file_output() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("json_test.log");

        let file = File::create(&log_path).unwrap();
        let layer = tracing_subscriber::fmt::layer()
            .event_format(RedactingJsonFormatter)
            .with_writer(Mutex::new(file));

        let subscriber = Registry::default().with(layer);

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(
                user_id = "user-42",
                api_key = "sk_live_abc123",
                "test event"
            );
        });

        let contents = fs::read_to_string(&log_path).unwrap();
        assert!(contents.contains("user-42"));
        assert!(contents.contains("test event"));
        assert!(
            !contents.contains("sk_live_abc123"),
            "api_key should be redacted in JSON output, but log contains: {}",
            contents
        );
        assert!(
            contents.contains("<redacted>"),
            "should contain <redacted> placeholder in JSON, but log contains: {}",
            contents
        );
    }

    #[test]
    fn no_secret_in_logs_plain_output_redacts() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("plain_redact_test.log");

        let file = File::create(&log_path).unwrap();
        let layer = tracing_subscriber::fmt::layer()
            .event_format(RedactingFormatter)
            .with_writer(Mutex::new(file));

        let subscriber = Registry::default().with(layer);

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(
                user_id = "user-42",
                password = "super_secret_123",
                "login attempt"
            );
        });

        let contents = fs::read_to_string(&log_path).unwrap();
        assert!(
            !contents.contains("super_secret_123"),
            "password value should be redacted, but log contains: {}",
            contents
        );
        assert!(
            contents.contains("<redacted>"),
            "should contain <redacted> placeholder, but log contains: {}",
            contents
        );
        assert!(
            contents.contains("user-42"),
            "user_id should be visible, but log contains: {}",
            contents
        );
    }

    // -----------------------------------------------------------------------
    // Panic hook tests

    #[test]
    fn panic_hook_restores_terminal() {
        let restored = Arc::new(AtomicBool::new(false));
        let restored_clone = Arc::clone(&restored);

        set_terminal_restore_hook(Box::new(move || {
            restored_clone.store(true, Ordering::SeqCst);
        }));

        let hook_lock = get_terminal_restore_hook();
        let hook_guard = hook_lock.lock().unwrap();
        assert!(
            hook_guard.is_some(),
            "terminal restore hook should be registered"
        );

        if let Some(ref restore_fn) = *hook_guard {
            restore_fn();
        }
        drop(hook_guard);

        assert!(
            restored.load(Ordering::SeqCst),
            "restore callback should have been invoked"
        );
    }
}
