//! Runtime context from herdr's environment, the press queue, and logging.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

pub const PLUGIN_ID: &str = "dev.panemorph";
pub const LOG_LINES: usize = 500;

/// What herdr told this process about where it runs.
#[derive(Debug, Clone, Default)]
pub struct Context {
    pub plugin_id: String,
    pub state_dir: PathBuf,
    /// The pane that was focused when the key was pressed (edge case 1.12).
    pub focused_pane_id: Option<String>,
}

impl Context {
    pub fn from_env() -> Self {
        let context_json: Value = std::env::var("HERDR_PLUGIN_CONTEXT_JSON")
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or(Value::Null);
        // Windows receive the source pane from the action that opened them;
        // actions read herdr's invocation context.
        let focused_pane_id = non_empty_env("PANEMORPH_SOURCE_PANE_ID")
            .or_else(|| {
                context_json
                    .get("focused_pane_id")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            })
            .or_else(|| non_empty_env("HERDR_PANE_ID"));
        Self {
            plugin_id: non_empty_env("HERDR_PLUGIN_ID").unwrap_or_else(|| PLUGIN_ID.to_string()),
            state_dir: state_dir(),
            focused_pane_id,
        }
    }
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// `HERDR_PLUGIN_STATE_DIR`, else `$XDG_STATE_HOME/panemorph`, else
/// `~/.local/state/panemorph`.
pub fn state_dir() -> PathBuf {
    if let Some(dir) = non_empty_env("HERDR_PLUGIN_STATE_DIR") {
        return PathBuf::from(dir);
    }
    if let Some(dir) = non_empty_env("XDG_STATE_HOME") {
        return PathBuf::from(dir).join("panemorph");
    }
    match non_empty_env("HOME") {
        Some(home) => PathBuf::from(home).join(".local/state/panemorph"),
        None => std::env::temp_dir().join("panemorph"),
    }
}

/// herdr's config file, read-only: `HERDR_CONFIG_PATH`, else
/// `$XDG_CONFIG_HOME/herdr/config.toml`, else `~/.config/herdr/config.toml`.
fn herdr_config_text() -> Option<String> {
    let path = non_empty_env("HERDR_CONFIG_PATH")
        .map(PathBuf::from)
        .or_else(|| {
            non_empty_env("XDG_CONFIG_HOME").map(|d| PathBuf::from(d).join("herdr/config.toml"))
        })
        .or_else(|| {
            non_empty_env("HOME").map(|h| PathBuf::from(h).join(".config/herdr/config.toml"))
        })?;
    std::fs::read_to_string(path).ok()
}

/// Does herdr capture the mouse (`[ui] mouse_capture`, default true)?
///
/// herdr's client holds a lone Esc for up to 150 ms while the host
/// terminal's mouse capture is on (herdr `raw_input.rs`,
/// MOUSE_ACTIVE_ESCAPE_SEQUENCE_FLUSH_TIMEOUT_MS), and 10 ms otherwise. A
/// window that asks for mouse reports turns capture on, so the windows only
/// use the mouse when herdr itself does.
pub fn herdr_mouse_capture() -> bool {
    herdr_config_text()
        .and_then(|text| config_value(&text, "ui.mouse_capture"))
        .map(|value| value != "false")
        .unwrap_or(true)
}

/// Does herdr deliver toasts (`[ui.toast] delivery`, default "off")?
///
/// On herdr 0.9.0 the viewing client applies this setting, so
/// `notification.show` answers `shown: true` whenever a client is attached,
/// even when the client then drops the toast (measured; see
/// docs/verification.md). paneMorph therefore reads the setting itself
/// (edge case 7.6).
pub fn herdr_toasts_on() -> bool {
    herdr_config_text()
        .and_then(|text| config_value(&text, "ui.toast.delivery"))
        .is_some_and(|value| value != "off")
}

/// Find a dotted key (e.g. `ui.toast.delivery`) in herdr's TOML, honouring
/// `[table]` headers and dotted keys. Returns the value without quotes.
pub fn config_value(text: &str, wanted: &str) -> Option<String> {
    let mut table = String::new();
    let mut found = None;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') {
            table = line
                .trim_matches(|c| c == '[' || c == ']')
                .trim()
                .to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key: String = key.split('.').map(str::trim).collect::<Vec<_>>().join(".");
        let full = if table.is_empty() {
            key
        } else {
            format!("{table}.{key}")
        };
        if full == wanted {
            found = Some(
                value
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .to_string(),
            );
        }
    }
    found
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Serialises paneMorph's moves (edge case 4.8). The OS releases the lock
/// when the process exits, even after a crash (edge case 7.12).
pub struct QueueLock {
    _file: File,
}

impl QueueLock {
    pub fn acquire(state_dir: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(state_dir)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(state_dir.join("queue.lock"))?;
        file.lock()?;
        Ok(Self { _file: file })
    }
}

/// Append one line to `panemorph.log`, keeping the last 500 lines (7.8).
pub fn append_log(state_dir: &Path, line: &str) {
    let _ = append_log_inner(state_dir, line);
}

fn append_log_inner(state_dir: &Path, line: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(state_dir)?;
    let path = state_dir.join("panemorph.log");
    let stamp = now_ms();
    {
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        writeln!(file, "{stamp} {line}")?;
    }
    let text = std::fs::read_to_string(&path)?;
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() > LOG_LINES {
        let keep = lines[lines.len() - LOG_LINES..].join("\n");
        let tmp = path.with_extension("log.tmp");
        std::fs::write(&tmp, format!("{keep}\n"))?;
        std::fs::rename(tmp, path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn herdr_settings_are_read_from_its_config() {
        let mouse =
            |text: &str| config_value(text, "ui.mouse_capture").is_none_or(|v| v != "false");
        assert!(mouse(""));
        assert!(mouse("[ui]\nsidebar = true\n"));
        assert!(!mouse(
            "onboarding = false\n[ui]\nmouse_capture = false # keyboard only\n"
        ));
        assert!(!mouse("ui.mouse_capture = false\n"));
        assert!(mouse("[ui.toast]\nmouse_capture = false\n"));
        let delivery = |text: &str| config_value(text, "ui.toast.delivery");
        assert_eq!(
            delivery("[keys]\nprefix = \"ctrl+b\"\n"),
            None,
            "default: off"
        );
        assert_eq!(
            delivery("[ui.toast]\ndelivery = \"herdr\"\n").as_deref(),
            Some("herdr")
        );
        assert_eq!(
            delivery("[ui]\ntoast.delivery = 'system'\n").as_deref(),
            Some("system")
        );
        assert_eq!(delivery("[ui.toast.herdr]\ndelivery = \"x\"\n"), None);
    }

    /// Edge case 7.8: the window log keeps its last 500 lines.
    #[test]
    fn edge_7_8_log_keeps_last_500_lines() {
        let dir = std::env::temp_dir().join(format!("pm-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for n in 0..520 {
            append_log(&dir, &format!("line {n}"));
        }
        let text = std::fs::read_to_string(dir.join("panemorph.log")).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), LOG_LINES);
        assert!(lines[0].ends_with("line 20"));
        assert!(lines.last().unwrap().ends_with("line 519"));
    }

    /// Edge cases 4.8 and 7.12: the lock serialises and is released on drop
    /// (process exit releases it the same way).
    #[test]
    fn edge_4_8_and_7_12_queue_lock_serialises_and_releases() {
        let dir = std::env::temp_dir().join(format!("pm-lock-{}", std::process::id()));
        let first = QueueLock::acquire(&dir).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let dir2 = dir.clone();
        let waiter = std::thread::spawn(move || {
            let _second = QueueLock::acquire(&dir2).unwrap();
            tx.send(()).unwrap();
        });
        assert!(rx
            .recv_timeout(std::time::Duration::from_millis(150))
            .is_err());
        drop(first);
        rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        waiter.join().unwrap();
    }
}
