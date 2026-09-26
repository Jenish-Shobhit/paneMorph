//! Drive the real binary's windows through a pseudo-terminal.
//!
//! `panemorph preview` runs the Send or Fetch window over the simulated
//! session, so these tests exercise real key bytes, crossterm's parser,
//! rendering and exit timing without touching any herdr server.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, Child, CommandBuilder, PtySize};
use serde_json::Value;

struct Window {
    parser: Arc<Mutex<vt100::Parser>>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    _master: Box<dyn portable_pty::MasterPty + Send>,
}

fn result_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("pm-pty-{name}-{}.json", std::process::id()))
}

impl Window {
    fn open(args: &[&str], rows: u16, cols: u16) -> Self {
        Self::open_with(args, rows, cols, &[])
    }

    fn open_with(args: &[&str], rows: u16, cols: u16, env: &[(&str, &str)]) -> Self {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("openpty");
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_panemorph"));
        command.args(args);
        command.env("TERM", "xterm-256color");
        // Nothing from a surrounding herdr session reaches the child.
        for (key, _) in std::env::vars() {
            if key.starts_with("HERDR_") {
                command.env_remove(key);
            }
        }
        // Never read the user's herdr config: default herdr settings.
        command.env(
            "HERDR_CONFIG_PATH",
            "/nonexistent/panemorph-test/config.toml",
        );
        for (key, value) in env {
            command.env(key, value);
        }
        let child = pair.slave.spawn_command(command).expect("spawn preview");
        drop(pair.slave);
        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));
        let mut reader = pair.master.try_clone_reader().expect("reader");
        let sink = parser.clone();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => sink.lock().unwrap().process(&buffer[..n]),
                }
            }
        });
        let writer = pair.master.take_writer().expect("writer");
        Self {
            parser,
            writer,
            child,
            _master: pair.master,
        }
    }

    /// One line per terminal row (vt100's `contents()` joins rows it
    /// considers wrapped, which hides full-width selected rows).
    fn screen(&self) -> String {
        let parser = self.parser.lock().unwrap();
        let screen = parser.screen();
        let (_, cols) = screen.size();
        screen
            .rows(0, cols)
            .map(|row| row.trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn wait_for(&self, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if self.screen().contains(needle) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{needle:?} never appeared:\n{}", self.screen());
    }

    fn wait_gone(&self, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if !self.screen().contains(needle) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{needle:?} never went away:\n{}", self.screen());
    }

    fn keys(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).unwrap();
        self.writer.flush().unwrap();
    }

    fn type_text(&mut self, text: &str) {
        for byte in text.bytes() {
            self.keys(&[byte]);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Time from the write until the process exits.
    fn exit_after(&mut self, bytes: &[u8]) -> Duration {
        let start = Instant::now();
        self.keys(bytes);
        loop {
            if self.child.try_wait().unwrap().is_some() {
                return start.elapsed();
            }
            if start.elapsed() > Duration::from_secs(5) {
                let _ = self.child.kill();
                panic!("window did not exit:\n{}", self.screen());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn row_of(&self, needle: &str) -> u16 {
        self.screen()
            .lines()
            .position(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} not on screen:\n{}", self.screen()))
            as u16
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

const ESC: &[u8] = b"\x1b";
const RIGHT: &[u8] = b"\x1b[C";
const LEFT: &[u8] = b"\x1b[D";
const DOWN: &[u8] = b"\x1b[B";
const ENTER: &[u8] = b"\r";

fn read_result(path: &PathBuf) -> Value {
    let text = std::fs::read_to_string(path).expect("preview wrote a result");
    let _ = std::fs::remove_file(path);
    serde_json::from_str(&text).unwrap()
}

#[test]
fn send_window_renders_filters_and_drills_in() {
    let mut w = Window::open(&["preview", "send"], 16, 72);
    w.wait_for("moving ○ claude");
    let screen = w.screen();
    for text in [
        "› filter tabs and spaces",
        "＋ New tab here",
        "named claude, next to this tab",
        "＋ New space",
        "named acme-web",
        "this space · Studio",
        "Landing_Page_Copy",
        "API refactor",
        "other spaces",
        "Payments_Service_Rewrite",
        "⏎ send",
        "lands: right",
    ] {
        assert!(screen.contains(text), "{text:?} missing:\n{screen}");
    }
    // Typing narrows every list at once.
    w.type_text("paym");
    w.wait_gone("API refactor");
    assert!(w.screen().contains("Payments_Service_Rewrite"));
    // → opens the space, ← goes back.
    w.keys(RIGHT);
    w.wait_for("← back");
    w.wait_for("New tab in Payments_Service_Rewrite");
    assert!(w.screen().contains("deploying"));
    w.keys(LEFT);
    w.wait_for("this space · Studio");
    // ⇥ toggles where the pane lands.
    w.keys(b"\t");
    w.wait_for("lands: below");
    assert!(w.exit_after(ESC) < Duration::from_secs(1));
}

/// Edge case 6.11: ⎋ closes at once. Measured over several runs; each must
/// close in under 100 ms (curses needed 1,021 ms).
#[test]
fn edge_6_11_escape_closes_under_100ms() {
    let mut worst = Duration::ZERO;
    let mut all = Vec::new();
    for mode in ["send", "fetch", "send", "fetch", "send"] {
        let mut w = Window::open(&["preview", mode], 16, 72);
        w.wait_for("› filter");
        std::thread::sleep(Duration::from_millis(50));
        let elapsed = w.exit_after(ESC);
        all.push(elapsed.as_millis());
        worst = worst.max(elapsed);
    }
    eprintln!("esc-to-exit ms: {all:?}");
    assert!(worst < Duration::from_millis(100), "esc took {all:?} ms");
}

/// Edge case 6.13: ⌃C closes like ⎋ and nothing moves.
#[test]
fn edge_6_13_ctrl_c_closes_without_moving() {
    let path = result_path("ctrlc");
    let mut w = Window::open(
        &["preview", "send", "--result", path.to_str().unwrap()],
        16,
        72,
    );
    w.wait_for("› filter");
    w.exit_after(b"\x03");
    let result = read_result(&path);
    assert_eq!(result["exit"], "closed");
    assert!(!result["calls"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c == "pane.move"));
}

/// ⏎ on a tab row sends the pane there, and the window exits (6.2).
#[test]
fn enter_sends_to_a_tab_and_closes() {
    let path = result_path("send");
    let mut w = Window::open(
        &["preview", "send", "--result", path.to_str().unwrap()],
        16,
        72,
    );
    w.wait_for("› filter");
    w.type_text("load");
    w.wait_gone("API refactor");
    w.exit_after(ENTER);
    let result = read_result(&path);
    assert_eq!(result["exit"], "moved");
    assert_eq!(result["summary"], "Sent claude to Load tests");
    let panes = result["snapshot"]["panes"].as_array().unwrap();
    let moved = panes
        .iter()
        .find(|p| p["label"] == "portfolio copy")
        .unwrap();
    let job = result["snapshot"]["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["label"] == "Load tests")
        .unwrap();
    assert_eq!(moved["tab_id"], job["tab_id"]);
}

/// Edge case 4.10: inside Send, ⌃⌥F (ESC ^F) switches to Fetch; ⌃⌥S and
/// ⌃⌥T are ignored. In preview the switch happens in place.
#[test]
fn edge_4_10_ctrl_alt_chords_inside_a_window() {
    let mut w = Window::open(&["preview", "send"], 16, 72);
    w.wait_for("moving");
    w.keys(b"\x1b\x13"); // ⌃⌥S: the window's own key
    w.keys(b"\x1b\x14"); // ⌃⌥T: another paneMorph chord
    std::thread::sleep(Duration::from_millis(150));
    assert!(w.screen().contains("moving"), "still Send:\n{}", w.screen());
    w.keys(b"\x1b\x06"); // ⌃⌥F
    w.wait_for("fetch into Landing_Page_Copy");
    w.keys(b"\x1b\x13"); // ⌃⌥S back to Send
    w.wait_for("moving");
    // ⌃⌥← arrives as CSI 1;7D and is ignored too.
    w.keys(b"\x1b[1;7D");
    std::thread::sleep(Duration::from_millis(100));
    assert!(w.screen().contains("moving"));
    w.exit_after(ESC);
}

/// Fetch: every pane by name, ⏎ on a tab row fetches the whole tab.
#[test]
fn fetch_window_lists_panes_and_fetches_a_whole_tab() {
    let path = result_path("fetch");
    let mut w = Window::open(
        &["preview", "fetch", "--result", path.to_str().unwrap()],
        20,
        76,
    );
    w.wait_for("fetch into Landing_Page_Copy · Studio, right of portfolio copy");
    // The running command replaces "shell" once process info arrives.
    w.wait_for("python3 scrape_docs.py");
    let screen = w.screen();
    assert!(screen.contains("⏎ whole tab"), "{screen}");
    assert!(!screen.contains("w1:p"), "no raw ids:\n{screen}");
    w.type_text("deploying");
    w.wait_gone("API refactor");
    w.exit_after(ENTER);
    let result = read_result(&path);
    assert_eq!(result["exit"], "moved");
    assert_eq!(result["summary"], "Fetched deploying (1 pane)");
}

/// Edge case 6.18: a double click acts on the row under the mouse.
#[test]
fn edge_6_18_double_click_acts() {
    let path = result_path("mouse");
    let mut w = Window::open(
        &["preview", "send", "--result", path.to_str().unwrap()],
        16,
        72,
    );
    w.wait_for("API refactor");
    let row = w.row_of("API refactor") + 1;
    let click = format!("\x1b[<0;10;{row}M\x1b[<0;10;{row}m");
    w.keys(click.as_bytes());
    std::thread::sleep(Duration::from_millis(40));
    w.exit_after(click.as_bytes());
    let result = read_result(&path);
    assert_eq!(result["summary"], "Sent claude to API refactor");
}

/// Edge case 6.14: a resize redraws and keeps the filter.
#[test]
fn edge_6_14_resize_keeps_filter() {
    let mut w = Window::open(&["preview", "send"], 16, 72);
    w.wait_for("› filter");
    w.type_text("res");
    w.wait_for("› res");
    w._master
        .resize(PtySize {
            rows: 24,
            cols: 90,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    std::thread::sleep(Duration::from_millis(150));
    {
        let mut parser = w.parser.lock().unwrap();
        parser.set_size(24, 90);
    }
    w.keys(DOWN);
    w.wait_for("› res");
    w.exit_after(ESC);
}

/// Edge case 6.5: the real window entrypoint with herdr unreachable shows
/// "Can't reach herdr" inline and waits for ⎋.
#[test]
fn edge_6_5_window_without_herdr_says_so_and_waits() {
    let state = std::env::temp_dir().join(format!("pm-pty-state-{}", std::process::id()));
    let mut w = Window::open_with(
        &["window", "send"],
        14,
        70,
        &[
            (
                "HERDR_SOCKET_PATH",
                "/nonexistent/panemorph-test/herdr.sock",
            ),
            ("HERDR_PLUGIN_STATE_DIR", state.to_str().unwrap()),
            ("PANEMORPH_SOURCE_PANE_ID", "w1:p1"),
        ],
    );
    w.wait_for("Can't reach herdr");
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        w.child.try_wait().unwrap().is_none(),
        "the window waits for ⎋"
    );
    assert!(w.exit_after(ESC) < Duration::from_secs(1));
    let _ = std::fs::remove_dir_all(state);
}

/// Print the windows as a terminal shows them (run with --ignored).
#[test]
#[ignore]
fn dump_screens() {
    for (mode, rows, cols) in [("send", 12, 46), ("send", 16, 72), ("fetch", 20, 76)] {
        let mut w = Window::open(&["preview", mode], rows, cols);
        w.wait_for("› filter");
        std::thread::sleep(Duration::from_millis(200));
        println!("--- {mode} {cols}x{rows}\n{}", w.screen());
        if mode == "send" && cols == 72 {
            w.type_text("paym");
            w.wait_gone("API refactor");
            w.keys(RIGHT);
            w.wait_for("← back");
            println!("--- send drilled\n{}", w.screen());
        }
        w.exit_after(ESC);
    }
}
