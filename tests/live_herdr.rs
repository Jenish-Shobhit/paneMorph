//! Live integration test on an isolated, throwaway herdr server.
//!
//! Off by default. Build the release binary first, then run:
//!
//!   cargo build --release
//!   PANEMORPH_LIVE=1 cargo test --release --test live_herdr -- --ignored --nocapture
//!
//! Isolation: the test starts its own herdr server with a private
//! `XDG_CONFIG_HOME` (so it has its own config.toml, plugins.json, sessions
//! and sockets), removes every inherited `HERDR_*` variable, registers this
//! checkout only in that private registry, and refuses to talk to any socket
//! outside its private directory. It never touches your herdr session.
//! Set `HERDR_BIN` to test another herdr binary and `PANEMORPH_LIVE_DIR` to
//! choose where the private directory goes (default /tmp; Unix socket paths
//! must stay under about 100 bytes).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use panemorph::api::{self, Herdr, SocketClient};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde_json::{json, Value};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

const KEYS: &[(&str, &str)] = &[
    ("ctrl+alt+s", "send"),
    ("ctrl+alt+f", "fetch"),
    ("ctrl+alt+t", "move-tab-new"),
    ("ctrl+alt+n", "move-space-new"),
    ("ctrl+alt+left", "move-tab-prev"),
    ("ctrl+alt+right", "move-tab-next"),
    ("ctrl+alt+z", "undo"),
];

fn chord(name: &str) -> &'static [u8] {
    match name {
        "s" => b"\x1b\x13",
        "f" => b"\x1b\x06",
        "t" => b"\x1b\x14",
        "n" => b"\x1b\x0e",
        "z" => b"\x1b\x1a",
        "left" => b"\x1b[1;7D",
        "right" => b"\x1b[1;7C",
        _ => unreachable!(),
    }
}

/// A private herdr server plus one attached client in a pty.
struct Session {
    dir: PathBuf,
    name: String,
    herdr: String,
    socket: PathBuf,
    client: SocketClient,
    server: Child,
    screen: Arc<Mutex<vt100::Parser>>,
    keys: Arc<Mutex<Box<dyn Write + Send>>>,
    viewer: Box<dyn portable_pty::Child + Send + Sync>,
    _master: Box<dyn portable_pty::MasterPty + Send>,
    handles: HashMap<&'static str, String>,
}

fn env_for(xdg: &Path) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = std::env::vars()
        .filter(|(k, _)| !k.starts_with("HERDR_") && !k.starts_with("XDG_"))
        .collect();
    // Config, state (plugin state dirs, agent-detection cache), data and
    // cache all live in the private directory.
    env.push(("XDG_CONFIG_HOME".into(), xdg.display().to_string()));
    for (key, sub) in [
        ("XDG_STATE_HOME", "state"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_CACHE_HOME", "cache"),
    ] {
        env.push((key.into(), xdg.join(sub).display().to_string()));
    }
    env.push(("TERM".into(), "xterm-256color".into()));
    env
}

fn registry_entry(root: &str) -> Value {
    let text = std::fs::read_to_string(Path::new(root).join("herdr-plugin.toml")).unwrap();
    let manifest: toml::Value = toml::from_str(&text).unwrap();
    let manifest = serde_json::to_value(manifest).unwrap();
    json!({
        "plugin_id": manifest["id"], "name": manifest["name"], "version": manifest["version"],
        "min_herdr_version": manifest["min_herdr_version"], "description": manifest["description"],
        "manifest_path": format!("{root}/herdr-plugin.toml"), "plugin_root": root, "enabled": true,
        "platforms": manifest["platforms"], "actions": manifest["actions"], "panes": manifest["panes"],
        "source": {"kind": "local"},
    })
}

impl Session {
    fn start() -> Self {
        let base = std::env::var("PANEMORPH_LIVE_DIR").unwrap_or_else(|_| "/tmp".into());
        let dir = PathBuf::from(base).join(format!("pmx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let home = dir.join("herdr");
        std::fs::create_dir_all(&home).unwrap();
        let mut config = String::from("onboarding = false\n");
        for (key, action) in KEYS {
            config.push_str(&format!(
                "\n[[keys.command]]\nkey = \"{key}\"\ntype = \"plugin_action\"\ncommand = \"dev.panemorph.{action}\"\n"
            ));
        }
        std::fs::write(home.join("config.toml"), config).unwrap();
        std::fs::write(
            home.join("plugins.json"),
            serde_json::to_vec_pretty(&json!([registry_entry(ROOT)])).unwrap(),
        )
        .unwrap();
        let name = format!("pmtest-{}", std::process::id());
        let herdr = std::env::var("HERDR_BIN").unwrap_or_else(|_| "herdr".into());
        let socket = home.join("sessions").join(&name).join("herdr.sock");
        assert!(
            socket.as_os_str().len() < 100,
            "socket path too long: {}",
            socket.display()
        );
        let server = Command::new(&herdr)
            .args(["--session", &name, "server"])
            .env_clear()
            .envs(env_for(&dir))
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start herdr (set HERDR_BIN if it is not on PATH)");
        let client = SocketClient::new(&socket);
        let deadline = Instant::now() + Duration::from_secs(15);
        while client.request("ping", json!({})).is_err() {
            assert!(Instant::now() < deadline, "isolated herdr did not start");
            std::thread::sleep(Duration::from_millis(100));
        }
        // Safety: the socket must live in the private directory.
        let real = std::fs::canonicalize(&socket).unwrap();
        assert!(
            real.starts_with(std::fs::canonicalize(&dir).unwrap()),
            "not isolated: {}",
            real.display()
        );
        let plugins = client.request("plugin.list", json!({})).unwrap();
        let roots: Vec<_> = plugins["plugins"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["plugin_root"].clone())
            .collect();
        assert_eq!(
            roots,
            vec![json!(ROOT)],
            "private registry must hold only this checkout"
        );

        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 40,
                cols: 150,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut viewer_cmd = CommandBuilder::new(&herdr);
        viewer_cmd.args(["--session", &name]);
        viewer_cmd.env_clear();
        for (k, v) in env_for(&dir) {
            viewer_cmd.env(k, v);
        }
        viewer_cmd.cwd(&dir);
        let viewer = pair.slave.spawn_command(viewer_cmd).unwrap();
        drop(pair.slave);
        let screen = Arc::new(Mutex::new(vt100::Parser::new(40, 150, 0)));
        let mut reader = pair.master.try_clone_reader().unwrap();
        let keys: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
        let replies = keys.clone();
        let sink = screen.clone();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 16384];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let chunk = &buffer[..n];
                        sink.lock().unwrap().process(chunk);
                        // Answer cursor and device queries so the client never waits.
                        let mut out = replies.lock().unwrap();
                        if chunk.windows(4).any(|w| w == b"\x1b[6n") {
                            let _ = out.write_all(b"\x1b[1;1R");
                        }
                        if chunk.windows(3).any(|w| w == b"\x1b[c") {
                            let _ = out.write_all(b"\x1b[?62;22c");
                        }
                    }
                }
            }
        });
        Self {
            dir,
            name,
            herdr,
            socket,
            client,
            server,
            screen,
            keys,
            viewer,
            _master: pair.master,
            handles: HashMap::new(),
        }
    }

    fn call(&self, method: &str, params: Value) -> Value {
        assert!(self.socket.starts_with(&self.dir));
        self.client
            .request(method, params)
            .unwrap_or_else(|e| panic!("{method}: {e}"))
    }

    fn snapshot(&self) -> panemorph::model::Snapshot {
        api::snapshot(&self.client).unwrap()
    }

    fn pane(&self, handle: &str) -> Option<panemorph::model::PaneInfo> {
        let terminal = &self.handles[handle];
        self.snapshot().pane_by_terminal(terminal).cloned()
    }

    fn id(&self, handle: &str) -> String {
        self.pane(handle).expect("pane is open").pane_id
    }

    /// (space label, tab label) holding a pane.
    fn place(&self, handle: &str) -> (String, String) {
        let snap = self.snapshot();
        let pane = snap
            .pane_by_terminal(&self.handles[handle])
            .expect("pane is open")
            .clone();
        (
            snap.workspace(&pane.workspace_id).unwrap().label.clone(),
            snap.tab(&pane.tab_id).unwrap().label.clone(),
        )
    }

    fn tabs(&self, space: &str) -> Vec<String> {
        let snap = self.snapshot();
        let ws = snap
            .workspaces
            .iter()
            .find(|w| w.label == space)
            .unwrap()
            .workspace_id
            .clone();
        snap.tabs_in(&ws).iter().map(|t| t.label.clone()).collect()
    }

    fn spaces(&self) -> Vec<String> {
        self.snapshot()
            .workspaces
            .iter()
            .map(|w| w.label.clone())
            .collect()
    }

    /// The split tree of a pane's tab, leaves named by handle.
    fn tree(&self, handle: &str) -> String {
        let tab = self.pane(handle).unwrap().tab_id;
        let root = self.call("layout.export", json!({"tab_id": tab}))["layout"]["root"].clone();
        let snap = self.snapshot();
        let names: HashMap<String, &str> = self
            .handles
            .iter()
            .filter_map(|(h, t)| snap.pane_by_terminal(t).map(|p| (p.pane_id.clone(), *h)))
            .collect();
        fn walk(node: &Value, names: &HashMap<String, &str>) -> String {
            if node["type"] == "pane" {
                names
                    .get(node["pane_id"].as_str().unwrap())
                    .unwrap_or(&"?")
                    .to_string()
            } else {
                format!(
                    "{}({:.2}: {}, {})",
                    node["direction"].as_str().unwrap(),
                    node["ratio"].as_f64().unwrap(),
                    walk(&node["first"], names),
                    walk(&node["second"], names)
                )
            }
        }
        walk(&root, &names)
    }

    fn zoomed(&self, handle: &str) -> bool {
        let tab = self.pane(handle).unwrap().tab_id;
        self.snapshot().layout(&tab).unwrap().zoomed
    }

    fn focused(&self) -> Option<String> {
        let snap = self.snapshot();
        let id = snap.focused_pane_id.clone()?;
        let terminal = snap.pane(&id)?.terminal_id.clone();
        self.handles
            .iter()
            .find(|(_, t)| **t == terminal)
            .map(|(h, _)| h.to_string())
    }

    fn press(&mut self, bytes: &[u8]) {
        let mut keys = self.keys.lock().unwrap();
        keys.write_all(bytes).unwrap();
        keys.flush().unwrap();
    }

    fn screen(&self) -> String {
        let parser = self.screen.lock().unwrap();
        parser.screen().rows(0, 150).collect::<Vec<_>>().join("\n")
    }

    fn window_open(&self, mode: &str) -> bool {
        let pattern = format!("{ROOT}/target/release/panemorph window {mode}");
        Command::new("pgrep")
            .args(["-f", &pattern])
            .output()
            .is_ok_and(|o| !o.stdout.is_empty())
    }

    /// The window and its worker exited and the client stopped drawing
    /// the popup; until then herdr routes keys to the closing popup.
    fn closed(&self) {
        let worker = format!("{ROOT}/target/release/panemorph apply");
        assert!(wait(|| {
            !self.window_open("send")
                && !self.window_open("fetch")
                && Command::new("pgrep")
                    .args(["-f", &worker])
                    .output()
                    .is_ok_and(|o| o.stdout.is_empty())
                && !self.screen().contains("paneMorph · ")
        }));
        std::thread::sleep(Duration::from_millis(60));
    }

    fn idle(&self) {
        wait(|| {
            self.call(
                "plugin.log.list",
                json!({"plugin_id": "dev.panemorph", "limit": 30}),
            )["logs"]
                .as_array()
                .unwrap()
                .iter()
                .all(|l| l["status"] != "running")
        });
        std::thread::sleep(Duration::from_millis(150));
    }

    fn open(&mut self, mode: &str) {
        self.press(chord(&mode[..1]));
        assert!(
            wait(|| self.window_open(mode)),
            "{mode} window did not open"
        );
        assert!(
            wait(|| self.screen().contains(&format!("paneMorph · {mode}"))),
            "{mode} not drawn"
        );
        std::thread::sleep(Duration::from_millis(250));
    }

    /// Type a marker through the client; return the handle that received it.
    fn typed_into(&mut self, marker: &str) -> Option<String> {
        self.press(format!("echo {marker}\r").as_bytes());
        let deadline = Instant::now() + Duration::from_secs(4);
        while Instant::now() < deadline {
            for handle in self.handles.keys() {
                if let Some(pane) = self.pane(handle) {
                    let text = self
                        .call(
                            "pane.read",
                            json!({"pane_id": pane.pane_id, "source": "recent", "lines": 60}),
                        )
                        .to_string();
                    if text.contains(marker) {
                        return Some(handle.to_string());
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        None
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.client.request("server.stop", json!({}));
        let _ = self.viewer.kill();
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.server.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.server.kill();
        let _ = std::fs::remove_dir_all(&self.dir);
        let _ = (&self.name, &self.herdr);
    }
}

fn wait(mut check: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    false
}

fn setup(s: &mut Session) {
    let cwd = |name: &str| {
        let dir = s.dir.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir.display().to_string()
    };
    let (app, solo, pair, ops) = (cwd("app"), cwd("solo"), cwd("pair"), cwd("ops"));
    let alpha = s.call(
        "workspace.create",
        json!({"cwd": app, "label": "alpha", "focus": true}),
    );
    let alpha_id = alpha["workspace"]["workspace_id"]
        .as_str()
        .unwrap()
        .to_string();
    s.call(
        "tab.rename",
        json!({"tab_id": alpha["tab"]["tab_id"], "label": "code"}),
    );
    let a1 = alpha["root_pane"].clone();
    let a2 = s.call("pane.split", json!({"target_pane_id": a1["pane_id"], "direction": "right", "ratio": 0.6, "cwd": app, "focus": false}))["pane"].clone();
    let logs = s.call(
        "tab.create",
        json!({"workspace_id": alpha_id, "cwd": solo, "label": "logs", "focus": false}),
    );
    let pair_tab = s.call(
        "tab.create",
        json!({"workspace_id": alpha_id, "cwd": pair, "label": "pair", "focus": false}),
    );
    let c2 = s.call("pane.split", json!({"target_pane_id": pair_tab["root_pane"]["pane_id"], "direction": "down", "ratio": 0.7, "cwd": pair, "focus": false}))["pane"].clone();
    let beta = s.call(
        "workspace.create",
        json!({"cwd": ops, "label": "beta", "focus": false}),
    );
    s.call(
        "tab.rename",
        json!({"tab_id": beta["tab"]["tab_id"], "label": "deploy"}),
    );
    for (handle, pane) in [
        ("A1", &a1),
        ("A2", &a2),
        ("B1", &logs["root_pane"]),
        ("C1", &pair_tab["root_pane"]),
        ("C2", &c2),
        ("D1", &beta["root_pane"]),
    ] {
        s.handles
            .insert(handle, pane["terminal_id"].as_str().unwrap().to_string());
    }
    std::thread::sleep(Duration::from_millis(1500));
    s.call(
        "pane.send_text",
        json!({"pane_id": s.id("A2"), "text": "sleep 7777\r"}),
    );
    s.call(
        "pane.send_text",
        json!({"pane_id": s.id("C2"), "text": "sleep 8888\r"}),
    );
    s.call("pane.focus", json!({"pane_id": s.id("A1")}));
    std::thread::sleep(Duration::from_millis(2500));
}

const START: &str = "right(0.60: A1, A2)";

#[test]
#[ignore = "live herdr test: PANEMORPH_LIVE=1 cargo test --release --test live_herdr -- --ignored"]
fn live_send_fetch_quick_keys_and_undo() {
    if std::env::var("PANEMORPH_LIVE").as_deref() != Ok("1") {
        eprintln!("skipped: set PANEMORPH_LIVE=1 to run against an isolated herdr");
        return;
    }
    assert!(
        Path::new(ROOT).join("target/release/panemorph").exists(),
        "run `cargo build --release` first"
    );
    let mut s = Session::start();
    setup(&mut s);
    let terminals: Vec<String> = s.handles.values().cloned().collect();

    // Send (1.1, 1.15, 1.16): window, filter, Enter; focus and the client follow.
    s.open("send");
    assert!(
        !s.snapshot().layouts.iter().any(|l| l.zoomed),
        "6.1: nothing zooms"
    );
    s.press(b"logs");
    std::thread::sleep(Duration::from_millis(250));
    s.press(b"\r");
    assert!(wait(|| s.place("A1") == ("alpha".into(), "logs".into())));
    s.closed();
    assert_eq!(s.tree("A1"), "right(0.50: B1, A1)");
    assert_eq!(s.focused().as_deref(), Some("A1"));
    assert_eq!(
        s.typed_into("PMLIVE1Q").as_deref(),
        Some("A1"),
        "the attached client follows"
    );

    // Undo (5.5, 5.4).
    s.press(chord("z"));
    assert!(wait(|| s.place("A1").1 == "code"));
    s.idle();
    assert_eq!(s.tree("A1"), START);

    // ⌃⌥T (1.2, 3.1, 3.6) and the alone no-op (4.5).
    s.call("pane.focus", json!({"pane_id": s.id("A2")}));
    std::thread::sleep(Duration::from_millis(300));
    s.press(chord("t"));
    assert!(wait(|| s.place("A2").1 == "sleep"));
    s.idle();
    assert_eq!(s.tabs("alpha"), ["code", "sleep", "logs", "pair"]);
    s.press(chord("t"));
    std::thread::sleep(Duration::from_millis(800));
    s.idle();
    assert_eq!(s.tabs("alpha"), ["code", "sleep", "logs", "pair"]);

    // ⌃⌥→ ⌃⌥→ ⌃⌥→ queue (4.8) and stop at the last tab (4.1); ⌃⌥← (4.2).
    for _ in 0..3 {
        s.press(chord("right"));
    }
    assert!(wait(|| s.place("A2").1 == "pair"));
    s.idle();
    s.press(chord("left"));
    assert!(wait(|| s.place("A2").1 == "logs"));
    s.idle();
    for _ in 0..4 {
        s.press(chord("z"));
        std::thread::sleep(Duration::from_millis(500));
        s.idle();
    }
    assert_eq!(s.tree("A1"), START, "5.3/5.7 undo chain");
    assert_eq!(s.tabs("alpha"), ["code", "logs", "pair"]);

    // ⌃⌥N (1.5, 3.3) and undo closing the new space.
    s.call("pane.focus", json!({"pane_id": s.id("A2")}));
    std::thread::sleep(Duration::from_millis(300));
    s.press(chord("n"));
    assert!(wait(|| s.place("A2").0 == "app"));
    s.idle();
    assert_eq!(s.spaces(), ["alpha", "beta", "app"]);
    s.press(chord("z"));
    assert!(wait(|| s.spaces() == ["alpha", "beta"]));
    s.idle();

    // Send into another space through drill-in (1.3, 1.18).
    s.call("pane.focus", json!({"pane_id": s.id("A2")}));
    std::thread::sleep(Duration::from_millis(300));
    let before = s.id("A2");
    s.open("send");
    s.press(b"beta");
    std::thread::sleep(Duration::from_millis(200));
    s.press(b"\x1b[C");
    assert!(wait(|| s.screen().contains("← back")));
    s.press(b"\x1b[B\r");
    assert!(wait(|| s.place("A2") == ("beta".into(), "deploy".into())));
    s.closed();
    assert_ne!(s.id("A2"), before);
    s.idle();
    s.press(chord("z"));
    assert!(wait(|| s.place("A2").1 == "code"));
    s.idle();
    assert_eq!(s.tree("A1"), START);

    // Fetch a pane (2.2) and a whole tab (2.4); undo both.
    s.call("pane.focus", json!({"pane_id": s.id("A1")}));
    std::thread::sleep(Duration::from_millis(300));
    s.open("fetch");
    assert!(
        wait(|| s.screen().contains("sleep 8888")),
        "2.12 commands fill in"
    );
    s.press(b"8888");
    std::thread::sleep(Duration::from_millis(300));
    s.press(b"\r");
    assert!(wait(|| s.place("C2").1 == "code"));
    s.closed();
    assert_eq!(s.tree("A1"), "right(0.60: right(0.50: A1, C2), A2)");
    assert_eq!(s.focused().as_deref(), Some("A1"));
    s.idle();
    s.press(chord("z"));
    assert!(wait(|| s.place("C2").1 == "pair"));
    s.idle();
    s.open("fetch");
    s.press(b"pair");
    std::thread::sleep(Duration::from_millis(300));
    s.press(b"\r");
    assert!(wait(|| s.place("C1").1 == "code"));
    s.closed();
    assert_eq!(
        s.tree("A1"),
        "right(0.60: right(0.50: A1, down(0.70: C1, C2)), A2)"
    );
    assert_eq!(s.tabs("alpha"), ["code", "logs"]);
    s.idle();
    s.press(chord("z"));
    assert!(wait(|| s.tabs("alpha") == ["code", "logs", "pair"]));
    s.idle();
    assert_eq!(s.tree("C1"), "down(0.70: C1, C2)", "5.10");

    // Zoomed source (1.11, 4.17) and re-zoom on undo (5.16).
    s.call("pane.focus", json!({"pane_id": s.id("A1")}));
    s.call("pane.zoom", json!({"pane_id": s.id("A1"), "mode": "on"}));
    std::thread::sleep(Duration::from_millis(300));
    s.press(chord("right"));
    assert!(wait(|| s.place("A1").1 == "logs"));
    s.idle();
    s.press(chord("z"));
    assert!(wait(|| s.place("A1").1 == "code"));
    s.idle();
    assert!(s.zoomed("A1"));
    s.call("pane.zoom", json!({"pane_id": s.id("A1"), "mode": "off"}));

    // Last pane in a tab (1.6) and recreating it on undo (5.7).
    s.call("pane.focus", json!({"pane_id": s.id("B1")}));
    std::thread::sleep(Duration::from_millis(300));
    s.press(chord("right"));
    assert!(wait(|| s.place("B1").1 == "pair"));
    s.idle();
    assert_eq!(s.tabs("alpha"), ["code", "pair"]);
    s.press(chord("z"));
    assert!(wait(|| s.tabs("alpha") == ["code", "logs", "pair"]));
    s.idle();

    // A second window request while one is open is refused by herdr and
    // logged as "window already open" (6.3).
    s.call("pane.focus", json!({"pane_id": s.id("A1")}));
    std::thread::sleep(Duration::from_millis(300));
    s.open("send");
    let invoked = s.call(
        "plugin.action.invoke",
        json!({"plugin_id": "dev.panemorph", "action_id": "send",
        "context": {"focused_pane_id": s.id("A1")}}),
    );
    let log_id = invoked["log"]["log_id"].as_str().unwrap().to_string();
    assert!(wait(|| s.call(
        "plugin.log.list",
        json!({"plugin_id": "dev.panemorph", "limit": 30})
    )["logs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["log_id"] == log_id.as_str()
            && l["status"] == "succeeded"
            && l["stdout"]
                .as_str()
                .unwrap_or("")
                .contains("window already open"))));
    s.press(b"\x1b");
    assert!(wait(|| !s.window_open("send")), "6.11 Esc closes");
    s.closed();

    // The deprecated alias still works and says so (4.15).
    s.call("pane.focus", json!({"pane_id": s.id("A2")}));
    let invoked = s.call(
        "plugin.action.invoke",
        json!({"plugin_id": "dev.panemorph", "action_id": "extract-pane",
        "context": {"focused_pane_id": s.id("A2")}}),
    );
    let log_id = invoked["log"]["log_id"].as_str().unwrap().to_string();
    assert!(wait(|| s.place("A2").1 == "sleep"));
    assert!(wait(|| s.call(
        "plugin.log.list",
        json!({"plugin_id": "dev.panemorph", "limit": 30})
    )["logs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["log_id"] == log_id.as_str()
            && l["stderr"]
                .as_str()
                .unwrap_or("")
                .contains("deprecated"))));
    s.idle();
    s.press(chord("z"));
    assert!(wait(|| s.place("A2").1 == "code"));
    s.idle();

    // Every terminal survived; the layout is back where it started (7.11).
    let snap = s.snapshot();
    for terminal in &terminals {
        assert!(
            snap.pane_by_terminal(terminal).is_some(),
            "terminal {terminal} still running"
        );
    }
    assert_eq!(s.tree("A1"), START);
    assert_eq!(s.tabs("alpha"), ["code", "logs", "pair"]);
    assert_eq!(s.spaces(), ["alpha", "beta"]);
    let failed: Vec<Value> = s.call(
        "plugin.log.list",
        json!({"plugin_id": "dev.panemorph", "limit": 200}),
    )["logs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["status"] == "failed")
        .cloned()
        .collect();
    assert!(failed.is_empty(), "7.10 failed actions: {failed:?}");
}
