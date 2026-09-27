//! Render the README screenshots from the real paneMorph windows.
//!
//! ```sh
//! cargo run --example screenshots             # windows over the demo session
//! cargo build --release
//! cargo run --example screenshots -- --live   # also a real herdr client
//! ```
//!
//! The default run draws the Send, Fetch and notice windows with the same
//! code the plugin runs (`ui::view::render`, `ui::notice::draw`), over the
//! simulated herdr session in `demo-session.json`, into ratatui's test
//! backend. It needs no herdr.
//!
//! `--live` also starts a throwaway herdr server with a private home and
//! config (see `live.rs`), attaches a real client in a pseudo-terminal,
//! opens Send and Fetch with their chords, and captures the client's
//! screen. It needs herdr 0.9 on `PATH` (or `HERDR_BIN`) and a release
//! build of this checkout.
//!
//! SVG files go to `docs/assets/` (or `--out DIR`).

mod live;
mod svg;

use std::path::{Path, PathBuf};

use panemorph::api::{self, Herdr};
use panemorph::names;
use panemorph::sim::Sim;
use panemorph::ui::app::{App, Input, Mode};
use panemorph::ui::{notice, view};
use ratatui::backend::{Backend, TestBackend};
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;
use unicode_width::UnicodeWidthStr;

use svg::{Cell, Grid, BG, FG};

const DEMO: &str = include_str!("demo-session.json");

fn color(color: Color, default: svg::Rgb) -> svg::Rgb {
    match color {
        Color::Reset => default,
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Indexed(index) => svg::indexed(index),
        Color::Black => svg::indexed(0),
        Color::Red => svg::indexed(1),
        Color::Green => svg::indexed(2),
        Color::Yellow => svg::indexed(3),
        Color::Blue => svg::indexed(4),
        Color::Magenta => svg::indexed(5),
        Color::Cyan => svg::indexed(6),
        Color::Gray => svg::indexed(7),
        Color::DarkGray => svg::indexed(8),
        Color::LightRed => svg::indexed(9),
        Color::LightGreen => svg::indexed(10),
        Color::LightYellow => svg::indexed(11),
        Color::LightBlue => svg::indexed(12),
        Color::LightMagenta => svg::indexed(13),
        Color::LightCyan => svg::indexed(14),
        Color::White => svg::indexed(15),
    }
}

/// A ratatui buffer as cells, keeping true colours and modifiers.
fn from_buffer(buffer: &Buffer) -> Grid {
    let area = buffer.area;
    let mut rows = Vec::new();
    for y in 0..area.height {
        let mut row: Vec<Cell> = Vec::new();
        let mut continuation = 0;
        for x in 0..area.width {
            let source = &buffer[(x, y)];
            let mut fg = color(source.fg, FG);
            let mut bg = color(source.bg, BG);
            if source.modifier.contains(Modifier::REVERSED) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if source.modifier.contains(Modifier::DIM) {
                fg = svg::mix(fg, bg, 0.45);
            }
            let symbol = source.symbol();
            row.push(Cell {
                text: if symbol.is_empty() { " " } else { symbol }.to_string(),
                fg,
                bg,
                bold: source.modifier.contains(Modifier::BOLD),
                italic: source.modifier.contains(Modifier::ITALIC),
                underline: source.modifier.contains(Modifier::UNDERLINED),
                continuation: continuation > 0,
            });
            continuation = if continuation > 0 {
                continuation - 1
            } else {
                symbol.width().saturating_sub(1)
            };
        }
        rows.push(row);
    }
    Grid { rows }
}

/// Draw one window over the demo session after the given keys.
fn window(mode: Mode, cols: u16, rows: u16, keys: &[Input]) -> Grid {
    let sim = Sim::from_json(DEMO).expect("demo-session.json parses");
    let snapshot = api::snapshot(&sim).expect("snapshot");
    let source = snapshot
        .focused_pane_id
        .as_deref()
        .and_then(|id| snapshot.pane(id))
        .cloned();
    let mut app = App::new(mode, snapshot.clone(), source);
    // What the window's background lookup fills in: the running command of
    // every pane without an agent.
    for pane in &snapshot.panes {
        if names::agent(pane).is_none() {
            if let Some(command) = api::process_info(&sim as &dyn Herdr, &pane.pane_id)
                .ok()
                .and_then(|info| info.command())
            {
                app.commands.insert(pane.terminal_id.clone(), command);
            }
        }
    }
    for key in keys {
        app.handle(*key);
    }
    let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("terminal");
    terminal
        .draw(|frame| view::render(frame, &mut app))
        .expect("draw");
    let cursor = terminal
        .backend_mut()
        .get_cursor_position()
        .expect("cursor");
    let mut grid = from_buffer(terminal.backend().buffer());
    grid.cursor(usize::from(cursor.x), usize::from(cursor.y));
    grid
}

fn notice_window(message: &str) -> Grid {
    // The manifest's notice popup is 60 × 7 with herdr's border.
    let mut terminal = Terminal::new(TestBackend::new(58, 5)).expect("terminal");
    terminal
        .draw(|frame| notice::draw(frame, message))
        .expect("draw");
    from_buffer(terminal.backend().buffer())
}

fn save(out: &Path, file: &str, title: &str, grid: &Grid) {
    let path = out.join(file);
    std::fs::write(&path, svg::render(grid, title)).expect("write svg");
    println!("wrote {}", path.display());
}

fn main() {
    let mut out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/assets");
    let mut live = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--live" => live = true,
            "--out" => out = PathBuf::from(args.next().expect("--out DIR")),
            other => {
                eprintln!("usage: cargo run --example screenshots -- [--live] [--out DIR]");
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }
    std::fs::create_dir_all(&out).expect("create output directory");

    let chars = |text: &str| text.chars().map(Input::Char).collect::<Vec<_>>();
    // Popup interiors for a 132 × 36 herdr client: Send is 60% × 50% and
    // Fetch 64% × 60%, less herdr's border.
    save(
        &out,
        "send.svg",
        "paneMorph · send",
        &window(Mode::Send, 78, 16, &[]),
    );
    let mut drill = chars("pay");
    drill.push(Input::Right);
    save(
        &out,
        "send-space.svg",
        "paneMorph · send",
        &window(Mode::Send, 78, 16, &drill),
    );
    save(
        &out,
        "fetch.svg",
        "paneMorph · fetch",
        &window(Mode::Fetch, 84, 23, &[]),
    );
    save(
        &out,
        "fetch-filter.svg",
        "paneMorph · fetch",
        &window(Mode::Fetch, 84, 12, &chars("claude")),
    );
    save(
        &out,
        "notice.svg",
        "paneMorph",
        &notice_window("Already the last tab"),
    );

    if live {
        let checkout = Path::new(env!("CARGO_MANIFEST_DIR"));
        match live::capture(checkout) {
            Ok(shots) => {
                for shot in shots {
                    save(&out, shot.file, "herdr", &shot.grid);
                }
            }
            Err(error) => {
                eprintln!("live capture failed: {error}");
                std::process::exit(1);
            }
        }
    }
}
