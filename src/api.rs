//! Newline-delimited JSON client for herdr's Unix socket API.
//!
//! Every request opens its own connection, writes one JSON line and reads one
//! JSON line back, exactly like herdr's own CLI. `events.subscribe` keeps its
//! connection open and streams one JSON line per event.

use std::fmt;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use serde_json::{json, Value};

use crate::model::{
    LayoutDescription, MoveResult, PaneInfo, ProcessInfo, Snapshot, SplitDir, TabInfo,
};

/// How a herdr request failed. The split between `Unreachable` and
/// `ReplyLost` matters for moves: a lost reply means the move may have
/// happened (edge case 6.6), so callers must re-read state before reporting.
#[derive(Debug, Clone, PartialEq)]
pub enum HerdrError {
    /// Could not connect or send; herdr never saw the request.
    Unreachable(String),
    /// The request was sent but no reply arrived.
    ReplyLost(String),
    /// herdr answered with an error object.
    Api { code: String, message: String },
    /// herdr answered with something paneMorph could not parse.
    Protocol(String),
}

impl HerdrError {
    pub fn api(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Api {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Api { code, .. } => Some(code),
            _ => None,
        }
    }

    pub fn is_transport(&self) -> bool {
        matches!(self, Self::Unreachable(_) | Self::ReplyLost(_))
    }

    /// True when herdr refused because another popup or modal is open.
    pub fn is_ui_busy(&self) -> bool {
        match self {
            Self::Api { code, message } => {
                code == "ui_busy"
                    || message.contains("popup already open")
                    || message.contains("popup pane is already open")
            }
            _ => false,
        }
    }
}

impl fmt::Display for HerdrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable(detail) => write!(f, "Can't reach herdr: {detail}"),
            Self::ReplyLost(detail) => write!(f, "herdr did not answer: {detail}"),
            Self::Api { code, message } => {
                if message.is_empty() {
                    write!(f, "{code}")
                } else {
                    write!(f, "{message}")
                }
            }
            Self::Protocol(detail) => write!(f, "Unexpected reply from herdr: {detail}"),
        }
    }
}

impl std::error::Error for HerdrError {}

/// Anything that can answer herdr socket requests: the real socket or the
/// in-memory simulator used by tests and `panemorph preview`.
pub trait Herdr: Send + Sync {
    fn request(&self, method: &str, params: Value) -> Result<Value, HerdrError>;

    /// Open an event stream. Each received line becomes one `()` on the
    /// channel. Returns `None` when the backend cannot stream events.
    fn subscribe(&self, _types: &[&str]) -> Option<Receiver<()>> {
        None
    }

    /// A stable identity for this herdr session (the socket path).
    fn session_key(&self) -> String;
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Real client over `HERDR_SOCKET_PATH`.
#[derive(Debug, Clone)]
pub struct SocketClient {
    path: PathBuf,
    timeout: Duration,
}

impl SocketClient {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            timeout: Duration::from_secs(10),
        }
    }

    pub fn from_env() -> Result<Self, HerdrError> {
        match std::env::var("HERDR_SOCKET_PATH") {
            Ok(path) if !path.is_empty() => Ok(Self::new(path)),
            _ => Err(HerdrError::Unreachable(
                "HERDR_SOCKET_PATH is not set; run paneMorph from herdr".into(),
            )),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn request_id() -> String {
        format!(
            "panemorph:{}:{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        )
    }

    fn open(&self) -> Result<UnixStream, HerdrError> {
        let stream = UnixStream::connect(&self.path).map_err(|error| {
            HerdrError::Unreachable(format!("{} ({})", error, self.path.display()))
        })?;
        stream
            .set_read_timeout(Some(self.timeout))
            .and_then(|()| stream.set_write_timeout(Some(self.timeout)))
            .map_err(|error| HerdrError::Unreachable(error.to_string()))?;
        Ok(stream)
    }
}

pub(crate) fn encode_request(id: &str, method: &str, params: &Value) -> Vec<u8> {
    let mut line = serde_json::to_vec(&json!({"id": id, "method": method, "params": params}))
        .expect("request JSON always serialises");
    line.push(b'\n');
    line
}

/// Parse one response line for request `id` into its `result` object.
pub(crate) fn decode_response(id: &str, line: &str) -> Result<Value, HerdrError> {
    let response: Value = serde_json::from_str(line.trim())
        .map_err(|error| HerdrError::Protocol(format!("invalid JSON: {error}")))?;
    if response.get("id").and_then(Value::as_str) != Some(id) {
        return Err(HerdrError::Protocol(
            "response belongs to another request".into(),
        ));
    }
    if let Some(error) = response.get("error") {
        let code = error
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("error")
            .to_string();
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| error.to_string());
        return Err(HerdrError::Api { code, message });
    }
    match response.get("result") {
        Some(result) if result.is_object() => Ok(result.clone()),
        _ => Err(HerdrError::Protocol("response has no result object".into())),
    }
}

impl Herdr for SocketClient {
    fn request(&self, method: &str, params: Value) -> Result<Value, HerdrError> {
        let id = Self::request_id();
        let mut stream = self.open()?;
        stream
            .write_all(&encode_request(&id, method, &params))
            .map_err(|error| HerdrError::Unreachable(error.to_string()))?;
        let mut line = String::new();
        let mut reader = BufReader::new(stream);
        match reader.read_line(&mut line) {
            Ok(0) => Err(HerdrError::ReplyLost(
                "herdr closed the socket without a reply".into(),
            )),
            Ok(_) => decode_response(&id, &line),
            Err(error) => Err(HerdrError::ReplyLost(error.to_string())),
        }
    }

    fn subscribe(&self, types: &[&str]) -> Option<Receiver<()>> {
        let id = Self::request_id();
        let subscriptions: Vec<Value> = types.iter().map(|t| json!({"type": t})).collect();
        let mut stream = self.open().ok()?;
        stream.set_read_timeout(None).ok()?;
        stream
            .write_all(&encode_request(
                &id,
                "events.subscribe",
                &json!({"subscriptions": subscriptions}),
            ))
            .ok()?;
        let mut reader = BufReader::new(stream);
        let mut ack = String::new();
        reader.read_line(&mut ack).ok()?;
        decode_response(&id, &ack).ok()?;
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if sender.send(()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Some(receiver)
    }

    fn session_key(&self) -> String {
        self.path.display().to_string()
    }
}

// ---------------------------------------------------------------------------
// Typed helpers shared by every backend.

fn field<T: serde::de::DeserializeOwned>(value: Value, key: &str) -> Result<T, HerdrError> {
    let inner = value
        .get(key)
        .cloned()
        .ok_or_else(|| HerdrError::Protocol(format!("reply is missing `{key}`")))?;
    serde_json::from_value(inner)
        .map_err(|error| HerdrError::Protocol(format!("bad `{key}`: {error}")))
}

/// Read calls are retried once after 250 ms (edge case 6.5); moves never are.
fn read_with_retry(herdr: &dyn Herdr, method: &str, params: Value) -> Result<Value, HerdrError> {
    match herdr.request(method, params.clone()) {
        Err(error) if error.is_transport() => {
            std::thread::sleep(Duration::from_millis(250));
            herdr.request(method, params)
        }
        other => other,
    }
}

pub fn snapshot(herdr: &dyn Herdr) -> Result<Snapshot, HerdrError> {
    field(
        read_with_retry(herdr, "session.snapshot", json!({}))?,
        "snapshot",
    )
}

/// Resolve a pane id, following herdr's aliases for panes that moved across
/// spaces (edge case 1.18).
pub fn pane_get(herdr: &dyn Herdr, pane_id: &str) -> Result<PaneInfo, HerdrError> {
    field(
        read_with_retry(herdr, "pane.get", json!({"pane_id": pane_id}))?,
        "pane",
    )
}

pub fn layout_export(herdr: &dyn Herdr, tab_id: &str) -> Result<LayoutDescription, HerdrError> {
    field(
        read_with_retry(herdr, "layout.export", json!({"tab_id": tab_id}))?,
        "layout",
    )
}

pub fn process_info(herdr: &dyn Herdr, pane_id: &str) -> Result<ProcessInfo, HerdrError> {
    field(
        read_with_retry(herdr, "pane.process_info", json!({"pane_id": pane_id}))?,
        "process_info",
    )
}

/// Destination of a `pane.move`.
#[derive(Debug, Clone, PartialEq)]
pub enum MoveDest {
    Tab {
        tab_id: String,
        target_pane_id: Option<String>,
        split: SplitDir,
        ratio: f64,
    },
    NewTab {
        workspace_id: String,
        label: Option<String>,
    },
    NewSpace {
        label: Option<String>,
        tab_label: Option<String>,
    },
}

impl MoveDest {
    pub fn to_json(&self) -> Value {
        match self {
            Self::Tab {
                tab_id,
                target_pane_id,
                split,
                ratio,
            } => {
                let mut value = json!({
                    "type": "tab",
                    "tab_id": tab_id,
                    "split": split.as_str(),
                    "ratio": ratio,
                });
                if let Some(target) = target_pane_id {
                    value["target_pane_id"] = json!(target);
                }
                value
            }
            Self::NewTab {
                workspace_id,
                label,
            } => json!({"type": "new_tab", "workspace_id": workspace_id, "label": label}),
            Self::NewSpace { label, tab_label } => {
                json!({"type": "new_workspace", "label": label, "tab_label": tab_label})
            }
        }
    }
}

pub fn pane_move(
    herdr: &dyn Herdr,
    pane_id: &str,
    dest: &MoveDest,
    focus: bool,
) -> Result<MoveResult, HerdrError> {
    field(
        herdr.request(
            "pane.move",
            json!({"pane_id": pane_id, "destination": dest.to_json(), "focus": focus}),
        )?,
        "move_result",
    )
}

pub fn tab_move(
    herdr: &dyn Herdr,
    tab_id: &str,
    insert_index: usize,
) -> Result<Vec<TabInfo>, HerdrError> {
    field(
        herdr.request(
            "tab.move",
            json!({"tab_id": tab_id, "insert_index": insert_index}),
        )?,
        "tabs",
    )
}

pub fn workspace_move(
    herdr: &dyn Herdr,
    workspace_id: &str,
    insert_index: usize,
) -> Result<(), HerdrError> {
    herdr
        .request(
            "workspace.move",
            json!({"workspace_id": workspace_id, "insert_index": insert_index}),
        )
        .map(|_| ())
}

/// `pane.zoom` with mode `on` or `off`. Returns whether the zoom changed.
pub fn pane_zoom(herdr: &dyn Herdr, pane_id: &str, on: bool) -> Result<bool, HerdrError> {
    let result = herdr.request(
        "pane.zoom",
        json!({"pane_id": pane_id, "mode": if on { "on" } else { "off" }}),
    )?;
    Ok(result
        .get("zoom")
        .and_then(|zoom| zoom.get("zoom_changed"))
        .and_then(Value::as_bool)
        .unwrap_or(false))
}

pub fn pane_swap(herdr: &dyn Herdr, source: &str, target: &str) -> Result<(), HerdrError> {
    herdr
        .request(
            "pane.swap",
            json!({"source_pane_id": source, "target_pane_id": target}),
        )
        .map(|_| ())
}

pub fn pane_focus(herdr: &dyn Herdr, pane_id: &str) -> Result<(), HerdrError> {
    herdr
        .request("pane.focus", json!({"pane_id": pane_id}))
        .map(|_| ())
}

/// `notification.show`. Returns `(shown, reason)`.
pub fn notify(herdr: &dyn Herdr, title: &str, body: &str) -> Result<(bool, String), HerdrError> {
    let result = herdr.request(
        "notification.show",
        json!({"title": title, "body": body, "sound": "none"}),
    )?;
    Ok((
        result
            .get("shown")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        result
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn encodes_one_newline_terminated_json_line() {
        let line = encode_request("r1", "ping", &json!({}));
        assert_eq!(line.last(), Some(&b'\n'));
        let value: Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(value["method"], "ping");
        assert_eq!(value["id"], "r1");
    }

    #[test]
    fn edge_6_3_and_6_4_ui_busy_is_recognised() {
        let error = decode_response(
            "r1",
            r#"{"id":"r1","error":{"code":"ui_busy","message":"a popup pane is already open"}}"#,
        )
        .unwrap_err();
        assert!(error.is_ui_busy());
        assert_eq!(error.code(), Some("ui_busy"));
    }

    #[test]
    fn mismatched_ids_are_rejected() {
        let error = decode_response("r1", r#"{"id":"r2","result":{}}"#).unwrap_err();
        assert!(matches!(error, HerdrError::Protocol(_)));
    }

    #[test]
    fn round_trip_over_a_real_unix_socket() {
        let dir = std::env::temp_dir().join(format!("pm-api-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.sock");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            let mut out = stream;
            let reply =
                json!({"id": request["id"], "result": {"type": "pong", "m": request["method"]}});
            out.write_all(format!("{reply}\n").as_bytes()).unwrap();
        });
        let client = SocketClient::new(&path);
        let result = client.request("ping", json!({})).unwrap();
        assert_eq!(result["m"], "ping");
        server.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_socket_is_unreachable_not_lost() {
        let client = SocketClient::new("/nonexistent/panemorph-test.sock");
        let error = client.request("ping", json!({})).unwrap_err();
        assert!(matches!(error, HerdrError::Unreachable(_)));
        assert!(error.to_string().starts_with("Can't reach herdr"));
    }
}
