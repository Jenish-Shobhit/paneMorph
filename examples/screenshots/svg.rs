//! Terminal cells to SVG, in a macOS-style window frame.
//!
//! The style is shared with codeMap's screenshots: Dracula colours on
//! #282a36, 14px "JetBrains Mono, SFMono-Regular, Menlo, Consolas,
//! monospace" at a 1.3 line height, three window dots, a centred title,
//! a 10px corner radius and a subtle border.
//!
//! Text is placed cell by cell, so the grid stays aligned whatever
//! monospace font the viewer has. Box-drawing and block characters are
//! drawn as vector shapes, so borders join without gaps between rows.

use std::fmt::Write as _;

use unicode_width::UnicodeWidthStr;

pub type Rgb = (u8, u8, u8);

/// Dracula background and foreground.
pub const BG: Rgb = (0x28, 0x2a, 0x36);
pub const FG: Rgb = (0xf8, 0xf8, 0xf2);

/// Dracula's ANSI palette, for indexed colours.
const ANSI: [Rgb; 16] = [
    (0x21, 0x22, 0x2c),
    (0xff, 0x55, 0x55),
    (0x50, 0xfa, 0x7b),
    (0xf1, 0xfa, 0x8c),
    (0xbd, 0x93, 0xf9),
    (0xff, 0x79, 0xc6),
    (0x8b, 0xe9, 0xfd),
    (0xf8, 0xf8, 0xf2),
    (0x62, 0x72, 0xa4),
    (0xff, 0x6e, 0x6e),
    (0x69, 0xff, 0x94),
    (0xff, 0xff, 0xa5),
    (0xd6, 0xac, 0xff),
    (0xff, 0x92, 0xdf),
    (0xa4, 0xff, 0xff),
    (0xff, 0xff, 0xff),
];

const FONT: &str = "JetBrains Mono, SFMono-Regular, Menlo, Consolas, monospace";
const FONT_SIZE: f64 = 14.0;
/// Advance of one cell: 0.6em, the width of most monospace fonts.
const CELL_W: f64 = 8.4;
/// Line height 1.3.
const CELL_H: f64 = 18.2;
/// Baseline offset inside a row.
const BASELINE: f64 = 13.6;
const PAD_X: f64 = 18.0;
const TITLE_BAR: f64 = 38.0;
const PAD_BOTTOM: f64 = 18.0;
const RADIUS: f64 = 10.0;
const BORDER: &str = "#44475a";
const TITLE_FG: &str = "#6272a4";

/// An xterm 256-colour index as RGB, with Dracula for the first 16.
pub fn indexed(index: u8) -> Rgb {
    match index {
        0..=15 => ANSI[usize::from(index)],
        16..=231 => {
            let i = index - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            (level(i / 36), level((i / 6) % 6), level(i % 6))
        }
        _ => {
            let v = 8 + (index - 232) * 10;
            (v, v, v)
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub text: String,
    pub fg: Rgb,
    pub bg: Rgb,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    /// The right half of a wide character: painted, never written.
    pub continuation: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            text: " ".into(),
            fg: FG,
            bg: BG,
            bold: false,
            italic: false,
            underline: false,
            continuation: false,
        }
    }
}

/// A screen of cells, one `Vec` per row.
pub struct Grid {
    pub rows: Vec<Vec<Cell>>,
}

impl Grid {
    pub fn cols(&self) -> usize {
        self.rows.first().map_or(0, Vec::len)
    }

    /// Draw a block cursor in Dracula's cursor colour.
    pub fn cursor(&mut self, x: usize, y: usize) {
        if let Some(cell) = self.rows.get_mut(y).and_then(|row| row.get_mut(x)) {
            cell.bg = FG;
            cell.fg = BG;
        }
    }
}

pub fn mix(a: Rgb, b: Rgb, amount: f64) -> Rgb {
    let m = |x: u8, y: u8| (f64::from(x) * (1.0 - amount) + f64::from(y) * amount).round() as u8;
    (m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

fn hex((r, g, b): Rgb) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Line segments of a box-drawing character: left, right, up, down, each
/// 0 (none), 1 (light) or 2 (heavy), plus whether corners are rounded.
fn box_segments(c: char) -> Option<([u8; 4], bool)> {
    let s = match c {
        '─' => [1, 1, 0, 0],
        '━' => [2, 2, 0, 0],
        '│' => [0, 0, 1, 1],
        '┃' => [0, 0, 2, 2],
        '┌' | '╭' => [0, 1, 0, 1],
        '┐' | '╮' => [1, 0, 0, 1],
        '└' | '╰' => [0, 1, 1, 0],
        '┘' | '╯' => [1, 0, 1, 0],
        '┏' => [0, 2, 0, 2],
        '┓' => [2, 0, 0, 2],
        '┗' => [0, 2, 2, 0],
        '┛' => [2, 0, 2, 0],
        '├' => [0, 1, 1, 1],
        '┤' => [1, 0, 1, 1],
        '┬' => [1, 1, 0, 1],
        '┴' => [1, 1, 1, 0],
        '┼' => [1, 1, 1, 1],
        '┣' => [0, 2, 2, 2],
        '┫' => [2, 0, 2, 2],
        '┳' => [2, 2, 0, 2],
        '┻' => [2, 2, 2, 0],
        '╋' => [2, 2, 2, 2],
        '╴' => [1, 0, 0, 0],
        '╶' => [0, 1, 0, 0],
        '╵' => [0, 0, 1, 0],
        '╷' => [0, 0, 0, 1],
        _ => return None,
    };
    Some((s, matches!(c, '╭' | '╮' | '╰' | '╯')))
}

/// Block elements as (x, y, w, h) fractions of the cell.
fn block_rect(c: char) -> Option<(f64, f64, f64, f64)> {
    Some(match c {
        '█' => (0.0, 0.0, 1.0, 1.0),
        '▌' => (0.0, 0.0, 0.5, 1.0),
        '▐' => (0.5, 0.0, 0.5, 1.0),
        '▀' => (0.0, 0.0, 1.0, 0.5),
        '▄' => (0.0, 0.5, 1.0, 0.5),
        '▏' => (0.0, 0.0, 0.125, 1.0),
        '▎' => (0.0, 0.0, 0.25, 1.0),
        '▍' => (0.0, 0.0, 0.375, 1.0),
        '▕' => (0.875, 0.0, 0.125, 1.0),
        _ => return None,
    })
}

/// Characters every monospace font has: drawn as runs of text. Anything
/// else is placed alone, centred in its cell, in case it falls back to
/// another font with a different advance.
fn plain(text: &str) -> bool {
    text.chars().all(|c| (c as u32) < 0x2000) && text.width() == 1
}

/// Render `grid` as an SVG document in a window titled `title`.
pub fn render(grid: &Grid, title: &str) -> String {
    let cols = grid.cols() as f64;
    let rows = grid.rows.len() as f64;
    let width = (cols * CELL_W + 2.0 * PAD_X).round();
    let height = (rows * CELL_H + TITLE_BAR + PAD_BOTTOM).round();
    let mut svg = String::new();
    let _ = write!(
        svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" role="img" aria-label="{label}">
<title>{label}</title>
<rect x="0.5" y="0.5" width="{w}" height="{h}" rx="{RADIUS}" fill="{bg}" stroke="{BORDER}"/>
<circle cx="20" cy="19" r="6" fill="#ff5f57"/><circle cx="40" cy="19" r="6" fill="#febc2e"/><circle cx="60" cy="19" r="6" fill="#28c840"/>
<text x="{cx}" y="23.5" text-anchor="middle" font-family="{FONT}" font-size="13" fill="{TITLE_FG}">{label}</text>
<g transform="translate({PAD_X} {TITLE_BAR})">
"##,
        label = escape(title),
        w = width - 1.0,
        h = height - 1.0,
        bg = hex(BG),
        cx = width / 2.0,
    );

    // Backgrounds: runs of one colour per row, snapped so neighbours
    // share edges without seams.
    svg.push_str(r#"<g shape-rendering="crispEdges">"#);
    for (y, row) in grid.rows.iter().enumerate() {
        let mut x = 0;
        while x < row.len() {
            let bg = row[x].bg;
            let start = x;
            while x < row.len() && row[x].bg == bg {
                x += 1;
            }
            if bg != BG {
                let _ = write!(
                    svg,
                    r#"<rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" fill="{}"/>"#,
                    start as f64 * CELL_W,
                    y as f64 * CELL_H,
                    (x - start) as f64 * CELL_W,
                    CELL_H,
                    hex(bg)
                );
            }
        }
    }
    svg.push_str("</g>\n");

    // Box drawing and blocks as shapes.
    svg.push_str(r#"<g shape-rendering="crispEdges" fill="none" stroke-linecap="square">"#);
    for (y, row) in grid.rows.iter().enumerate() {
        for (x, cell) in row.iter().enumerate() {
            let mut chars = cell.text.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else {
                continue;
            };
            let left = x as f64 * CELL_W;
            let top = y as f64 * CELL_H;
            if let Some((fx, fy, fw, fh)) = block_rect(c) {
                let _ = write!(
                    svg,
                    r#"<rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" fill="{}" stroke="none"/>"#,
                    left + fx * CELL_W,
                    top + fy * CELL_H,
                    fw * CELL_W,
                    fh * CELL_H,
                    hex(cell.fg)
                );
            } else if let Some((segments, rounded)) = box_segments(c) {
                let (cx, cy) = (left + CELL_W / 2.0, top + CELL_H / 2.0);
                let ends = [
                    (left, cy),
                    (left + CELL_W, cy),
                    (cx, top),
                    (cx, top + CELL_H),
                ];
                let heavy = segments.contains(&2);
                let stroke = if heavy { 2.0 } else { 1.0 };
                let mut d = String::new();
                if rounded {
                    // One horizontal and one vertical arm, joined by a curve.
                    let h = if segments[0] > 0 { ends[0] } else { ends[1] };
                    let v = if segments[2] > 0 { ends[2] } else { ends[3] };
                    let _ = write!(
                        d,
                        "M{:.2} {:.2}Q{cx:.2} {cy:.2} {:.2} {:.2}",
                        h.0, h.1, v.0, v.1
                    );
                } else {
                    for (i, &s) in segments.iter().enumerate() {
                        if s > 0 {
                            let _ = write!(d, "M{cx:.2} {cy:.2}L{:.2} {:.2}", ends[i].0, ends[i].1);
                        }
                    }
                }
                let _ = write!(
                    svg,
                    r#"<path d="{d}" stroke="{}" stroke-width="{stroke}"/>"#,
                    hex(cell.fg)
                );
            }
        }
    }
    svg.push_str("</g>\n");

    // Text.
    let _ = write!(svg, r#"<g font-family="{FONT}" font-size="{FONT_SIZE}">"#);
    for (y, row) in grid.rows.iter().enumerate() {
        let baseline = y as f64 * CELL_H + BASELINE;
        let mut x = 0;
        while x < row.len() {
            let cell = &row[x];
            let single = cell.text.chars().count() == 1;
            let shape = single
                && cell
                    .text
                    .chars()
                    .next()
                    .is_some_and(|c| block_rect(c).is_some() || box_segments(c).is_some());
            if cell.continuation || cell.text.trim().is_empty() || shape {
                x += 1;
                continue;
            }
            let style = attributes(cell);
            if plain(&cell.text) {
                // A run of plain cells in one style.
                let start = x;
                let mut text = String::new();
                while x < row.len() {
                    let next = &row[x];
                    // Two spaces end a run: renderers collapse runs of
                    // whitespace, so wide gaps are left to positioning.
                    let gap = next.text == " " && row.get(x + 1).is_none_or(|c| c.text == " ");
                    if next.continuation || !plain(&next.text) || attributes(next) != style || gap {
                        break;
                    }
                    text.push_str(&next.text);
                    x += 1;
                }
                let trimmed = text.trim_end();
                let cells = trimmed.chars().count();
                let _ = writeln!(
                    svg,
                    r#"<text x="{:.2}" y="{baseline:.2}" textLength="{:.2}" lengthAdjust="spacing" xml:space="preserve"{style}>{}</text>"#,
                    start as f64 * CELL_W,
                    cells as f64 * CELL_W,
                    escape(trimmed)
                );
            } else {
                let span = cell.text.width().max(1) as f64;
                let _ = writeln!(
                    svg,
                    r#"<text x="{:.2}" y="{baseline:.2}" text-anchor="middle"{style}>{}</text>"#,
                    (x as f64 + span / 2.0) * CELL_W,
                    escape(&cell.text)
                );
                x += 1;
            }
        }
    }
    svg.push_str("</g>\n</g>\n</svg>\n");
    svg
}

fn attributes(cell: &Cell) -> String {
    let mut out = format!(r#" fill="{}""#, hex(cell.fg));
    if cell.bold {
        out.push_str(r#" font-weight="bold""#);
    }
    if cell.italic {
        out.push_str(r#" font-style="italic""#);
    }
    if cell.underline {
        out.push_str(r#" text-decoration="underline""#);
    }
    out
}
