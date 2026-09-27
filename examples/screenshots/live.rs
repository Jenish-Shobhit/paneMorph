//! Capture a real herdr client with a paneMorph window open.
//!
//! Everything runs in a throwaway herdr session under a private directory:
//! its own `HOME`, `ZDOTDIR` and `XDG_{CONFIG,STATE,DATA,CACHE}_HOME`, so
//! it has its own config, plugin registry, sockets, shell prompt and state,
//! and no inherited `HERDR_*` variables. A private copy of the built plugin
//! is registered in that private registry only. The client runs in a
//! pseudo-terminal, keys are typed through it, and its output is read with
//! a VT100 parser. Nothing touches your own herdr session.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use panemorph::api::{Herdr, SocketClient};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde_json::{json, Value};

use crate::svg::{self, Cell, Grid, BG, FG};

pub const ROWS: u16 = 34;
pub const COLS: u16 = 128;

/// One captured screen and the file name it goes to.
pub struct Shot {
    pub file: &'static str,
    pub grid: Grid,
}

struct Session {
    dir: PathBuf,
    socket: PathBuf,
    client: SocketClient,
    server: Child,
    viewer: Box<dyn portable_pty::Child + Send + Sync>,
    screen: Arc<Mutex<vt100::Parser>>,
    keys: Arc<Mutex<Box<dyn Write + Send>>>,
    _master: Box<dyn portable_pty::MasterPty + Send>,
}

fn env_for(dir: &Path) -> Vec<(String, String)> {
    let home = dir.join("home").display().to_string();
    let mut env = vec![
        ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
        ("HOME".into(), home.clone()),
        ("ZDOTDIR".into(), home),
        ("SHELL".into(), "/bin/zsh".into()),
        ("TERM".into(), "xterm-256color".into()),
        ("COLORTERM".into(), "truecolor".into()),
        ("LANG".into(), "en_US.UTF-8".into()),
        ("LESS".into(), "-R".into()),
    ];
    for (key, sub) in [
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_STATE_HOME", "state"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_CACHE_HOME", "cache"),
    ] {
        env.push((key.into(), dir.join(sub).display().to_string()));
    }
    env
}

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, text).unwrap();
}

fn copy_file(from: &Path, to: &Path) {
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::copy(from, to).unwrap_or_else(|e| panic!("{}: {e}", from.display()));
}

/// The private plugin registry entry herdr would write for a linked plugin.
fn registry_entry(root: &Path) -> Value {
    let text = std::fs::read_to_string(root.join("herdr-plugin.toml")).unwrap();
    let manifest: toml::Value = toml::from_str(&text).unwrap();
    let manifest = serde_json::to_value(manifest).unwrap();
    json!({
        "plugin_id": manifest["id"], "name": manifest["name"], "version": manifest["version"],
        "min_herdr_version": manifest["min_herdr_version"], "description": manifest["description"],
        "manifest_path": root.join("herdr-plugin.toml"), "plugin_root": root, "enabled": true,
        "platforms": manifest["platforms"], "actions": manifest["actions"], "panes": manifest["panes"],
        "source": {"kind": "local"},
    })
}

fn wait(mut check: impl FnMut() -> bool, seconds: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// The server answers on the private socket and knows only the private
/// copy of the plugin.
fn check_isolation(
    client: &SocketClient,
    socket: &Path,
    dir: &Path,
    plugin: &Path,
) -> Result<(), String> {
    if !wait(|| client.request("ping", json!({})).is_ok(), 15) {
        return Err("the throwaway herdr server did not start".into());
    }
    let real = std::fs::canonicalize(socket).map_err(|e| e.to_string())?;
    if !real.starts_with(dir) {
        return Err(format!("not isolated: {}", real.display()));
    }
    let plugins = client
        .request("plugin.list", json!({}))
        .map_err(|e| e.to_string())?;
    let roots: Vec<Value> = plugins["plugins"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|p| p["plugin_root"].clone())
        .collect();
    if roots != vec![json!(plugin)] {
        return Err(format!("unexpected plugin registry: {roots:?}"));
    }
    Ok(())
}

/// A real herdr client attached in a pseudo-terminal.
struct Attached {
    viewer: Box<dyn portable_pty::Child + Send + Sync>,
    screen: Arc<Mutex<vt100::Parser>>,
    keys: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Box<dyn portable_pty::MasterPty + Send>,
}

fn attach(herdr: &str, name: &str, dir: &Path) -> Result<Attached, String> {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: ROWS,
            cols: COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;
    let mut command = CommandBuilder::new(herdr);
    command.args(["--session", name]);
    command.env_clear();
    for (key, value) in env_for(dir) {
        command.env(key, value);
    }
    command.cwd(dir);
    let viewer = pair
        .slave
        .spawn_command(command)
        .map_err(|e| e.to_string())?;
    drop(pair.slave);
    let screen = Arc::new(Mutex::new(vt100::Parser::new(ROWS, COLS, 0)));
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let keys: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(
        pair.master.take_writer().map_err(|e| e.to_string())?,
    ));
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
    Ok(Attached {
        viewer,
        screen,
        keys,
        master: pair.master,
    })
}

/// Tear down a throwaway server that never became a session. It has no
/// panes yet, so ending the process we spawned is enough; nothing is sent
/// to a socket that failed the isolation check.
fn abandon(mut server: Child, dir: &Path) {
    let _ = server.kill();
    let _ = server.wait();
    let _ = std::fs::remove_dir_all(dir);
}

impl Session {
    fn start(checkout: &Path) -> Result<Self, String> {
        let binary = checkout.join("target/release/panemorph");
        if !binary.exists() {
            return Err("run `cargo build --release` first".into());
        }
        // Unix socket paths must stay short, so the directory lives in /tmp.
        let dir = PathBuf::from("/tmp").join(format!("pmshot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let dir = std::fs::canonicalize(&dir).map_err(|e| e.to_string())?;

        // A private copy of the plugin: manifest, launcher and binary.
        let plugin = dir.join("plugin");
        copy_file(
            &checkout.join("herdr-plugin.toml"),
            &plugin.join("herdr-plugin.toml"),
        );
        copy_file(
            &checkout.join("bin/panemorph"),
            &plugin.join("bin/panemorph"),
        );
        copy_file(&binary, &plugin.join("target/release/panemorph"));

        let herdr_home = dir.join("config/herdr");
        let mut config = String::from("onboarding = false\n\n[theme]\nname = \"dracula\"\n");
        for (key, action) in [("ctrl+alt+s", "send"), ("ctrl+alt+f", "fetch")] {
            config.push_str(&format!(
                "\n[[keys.command]]\nkey = \"{key}\"\ntype = \"plugin_action\"\ncommand = \"dev.panemorph.{action}\"\n"
            ));
        }
        write(&herdr_home.join("config.toml"), &config);
        write(
            &herdr_home.join("plugins.json"),
            &serde_json::to_string_pretty(&json!([registry_entry(&plugin)])).unwrap(),
        );
        // A neutral prompt: the folder name only, never user or host.
        write(
            &dir.join("home/.zshrc"),
            "PROMPT='%F{magenta}%1~%f $ '\nRPROMPT=''\nunsetopt PROMPT_SP\n",
        );
        for (folder, files) in [
            (
                "acme-web",
                &["README.md", "TODO.md", "package.json", "tsconfig.json"][..],
            ),
            ("acme-api", &["README.md", "Cargo.toml"][..]),
            ("payments", &["README.md", "compose.yaml"][..]),
            ("docs", &["README.md", "config.toml"][..]),
        ] {
            for file in files {
                let text = match *file {
                    "README.md" => format!(
                        "# {folder}\n\nDemo project for the paneMorph screenshots.\n\n\
                         ## Develop\n\n    npm install\n    npm run dev\n\n\
                         ## Test\n\n    npm test\n"
                    ),
                    "TODO.md" => "# TODO\n\n- [ ] cart totals\n- [ ] coupon codes\n".to_string(),
                    _ => String::new(),
                };
                write(&dir.join("work").join(folder).join(file), &text);
            }
            std::fs::create_dir_all(dir.join("work").join(folder).join("src")).unwrap();
        }

        let herdr = std::env::var("HERDR_BIN").unwrap_or_else(|_| "herdr".into());
        let name = format!("shot-{}", std::process::id());
        let socket = herdr_home.join("sessions").join(&name).join("herdr.sock");
        let server = Command::new(&herdr)
            .args(["--session", &name, "server"])
            .env_clear()
            .envs(env_for(&dir))
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("start herdr (set HERDR_BIN if it is not on PATH): {e}"))?;
        let client = SocketClient::new(&socket);
        if let Err(error) = check_isolation(&client, &socket, &dir, &plugin) {
            abandon(server, &dir);
            return Err(error);
        }

        let client_pty = match attach(&herdr, &name, &dir) {
            Ok(attached) => attached,
            Err(error) => {
                abandon(server, &dir);
                return Err(error);
            }
        };
        Ok(Self {
            dir,
            socket,
            client,
            server,
            viewer: client_pty.viewer,
            screen: client_pty.screen,
            keys: client_pty.keys,
            _master: client_pty.master,
        })
    }

    fn call(&self, method: &str, params: Value) -> Value {
        assert!(self.socket.starts_with(&self.dir));
        self.client
            .request(method, params)
            .unwrap_or_else(|e| panic!("{method}: {e}"))
    }

    fn press(&self, bytes: &[u8]) {
        let mut keys = self.keys.lock().unwrap();
        keys.write_all(bytes).unwrap();
        keys.flush().unwrap();
    }

    fn text(&self) -> String {
        let parser = self.screen.lock().unwrap();
        parser.screen().rows(0, COLS).collect::<Vec<_>>().join("\n")
    }

    fn cwd(&self, folder: &str) -> String {
        self.dir.join("work").join(folder).display().to_string()
    }

    fn type_into(&self, pane: &Value, text: &str) {
        self.call(
            "pane.send_text",
            json!({"pane_id": pane["pane_id"], "text": text}),
        );
    }

    fn label(&self, pane: &Value, label: &str) {
        self.call(
            "pane.rename",
            json!({"pane_id": pane["pane_id"], "label": label}),
        );
    }

    /// The demo layout: three spaces, neutral names, idle shells and a few
    /// running commands.
    fn setup(&self) -> Value {
        let storefront = self.call(
            "workspace.create",
            json!({"cwd": self.cwd("acme-web"), "label": "storefront", "focus": true}),
        );
        let ws = storefront["workspace"]["workspace_id"].clone();
        self.call(
            "tab.rename",
            json!({"tab_id": storefront["tab"]["tab_id"], "label": "checkout"}),
        );
        let moving = storefront["root_pane"].clone();
        let readme = self.call(
            "pane.split",
            json!({"target_pane_id": moving["pane_id"], "direction": "right", "ratio": 0.55,
                   "cwd": self.cwd("acme-web"), "focus": false}),
        )["pane"]
            .clone();
        let api = self.call(
            "tab.create",
            json!({"workspace_id": ws, "cwd": self.cwd("acme-api"), "label": "api", "focus": false}),
        );
        let tests = self.call(
            "tab.create",
            json!({"workspace_id": ws, "cwd": self.cwd("acme-web"), "label": "tests", "focus": false}),
        );
        let payments = self.call(
            "workspace.create",
            json!({"cwd": self.cwd("payments"), "label": "payments", "focus": false}),
        );
        self.call(
            "tab.rename",
            json!({"tab_id": payments["tab"]["tab_id"], "label": "refunds"}),
        );
        let compose = self.call(
            "pane.split",
            json!({"target_pane_id": payments["root_pane"]["pane_id"], "direction": "down",
                   "ratio": 0.6, "cwd": self.cwd("payments"), "focus": false}),
        )["pane"]
            .clone();
        self.call(
            "tab.create",
            json!({"workspace_id": payments["workspace"]["workspace_id"], "cwd": self.cwd("payments"),
                   "label": "ledger", "focus": false}),
        );
        let docs = self.call(
            "workspace.create",
            json!({"cwd": self.cwd("docs"), "label": "docs", "focus": false}),
        );
        self.call(
            "tab.rename",
            json!({"tab_id": docs["tab"]["tab_id"], "label": "site"}),
        );
        // Let the shells draw their first prompt.
        std::thread::sleep(Duration::from_millis(2000));
        self.label(&moving, "checkout form");
        self.label(&api["root_pane"], "rate limiter");
        self.label(&payments["root_pane"], "refund webhooks");
        self.type_into(&moving, "ls\r");
        self.type_into(&readme, "less README.md\r");
        self.type_into(&tests["root_pane"], "vim TODO.md\r");
        self.type_into(&compose, "less compose.yaml\r");
        self.type_into(&docs["root_pane"], "less README.md\r");
        self.call("pane.focus", json!({"pane_id": moving["pane_id"]}));
        std::thread::sleep(Duration::from_millis(2500));
        moving
    }

    /// Open a window with its chord and wait until it is drawn.
    fn open(&self, chord: &[u8], title: &str, ready: &str) -> Result<(), String> {
        self.press(chord);
        if !wait(
            || self.text().contains(title) && self.text().contains(ready),
            10,
        ) {
            return Err(format!("{title} did not open:\n{}", self.text()));
        }
        std::thread::sleep(Duration::from_millis(800));
        Ok(())
    }

    fn close_window(&self, title: &str) {
        self.press(b"\x1b");
        wait(|| !self.text().contains(title), 5);
        std::thread::sleep(Duration::from_millis(300));
    }

    fn grid(&self) -> Grid {
        let parser = self.screen.lock().unwrap();
        from_screen(parser.screen())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Stop only this throwaway server, through its private socket.
        if self.socket.starts_with(&self.dir) {
            let _ = self.client.request("server.stop", json!({}));
        }
        let _ = self.viewer.kill();
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.server.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.server.kill();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn color(color: vt100::Color, default: svg::Rgb) -> svg::Rgb {
    match color {
        vt100::Color::Default => default,
        vt100::Color::Idx(index) => svg::indexed(index),
        vt100::Color::Rgb(r, g, b) => (r, g, b),
    }
}

fn from_screen(screen: &vt100::Screen) -> Grid {
    let (rows, cols) = screen.size();
    let mut grid = Grid {
        rows: (0..rows)
            .map(|row| {
                (0..cols)
                    .map(|col| {
                        let Some(cell) = screen.cell(row, col) else {
                            return Cell::default();
                        };
                        let mut fg = color(cell.fgcolor(), FG);
                        let mut bg = color(cell.bgcolor(), BG);
                        if cell.inverse() {
                            std::mem::swap(&mut fg, &mut bg);
                        }
                        if cell.dim() {
                            fg = svg::mix(fg, bg, 0.45);
                        }
                        let text = if cell.has_contents() {
                            cell.contents().to_string()
                        } else {
                            " ".to_string()
                        };
                        Cell {
                            text,
                            fg,
                            bg,
                            bold: cell.bold(),
                            italic: cell.italic(),
                            underline: cell.underline(),
                            continuation: cell.is_wide_continuation(),
                        }
                    })
                    .collect()
            })
            .collect(),
    };
    if !screen.hide_cursor() {
        let (row, col) = screen.cursor_position();
        grid.cursor(usize::from(col), usize::from(row));
    }
    grid
}

/// Words that must never appear in a capture: the real user and host.
fn private_words() -> Vec<String> {
    let mut words = vec!["/Users/".to_string(), "/home/".to_string()];
    for key in ["USER", "LOGNAME"] {
        if let Ok(value) = std::env::var(key) {
            if value.len() > 2 {
                words.push(value);
            }
        }
    }
    if let Ok(out) = Command::new("hostname").arg("-s").output() {
        let host = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if host.len() > 2 {
            words.push(host);
        }
    }
    words
}

/// Start the throwaway session, open Send, and return the screen.
pub fn capture(checkout: &Path) -> Result<Vec<Shot>, String> {
    let session = Session::start(checkout)?;
    session.setup();
    let mut shots = Vec::new();
    session.open(b"\x1b\x13", "paneMorph · send", "other spaces")?;
    shots.push(Shot {
        file: "hero.svg",
        grid: session.grid(),
    });
    session.close_window("paneMorph · send");
    drop(session);

    let private = private_words();
    for shot in &shots {
        let text: String = shot
            .grid
            .rows
            .iter()
            .flat_map(|row| row.iter().map(|c| c.text.as_str()))
            .collect();
        if let Some(word) = private.iter().find(|w| text.contains(w.as_str())) {
            return Err(format!(
                "{} would show private text ({word:?}); nothing written",
                shot.file
            ));
        }
    }
    Ok(shots)
}
