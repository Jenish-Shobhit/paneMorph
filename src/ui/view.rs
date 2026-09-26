//! Drawing the Send and Fetch windows with ratatui.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::names;
use crate::ui::app::{App, ListGeometry, Mode};
use crate::ui::rows::{Row, RowStyle};

pub const ACCENT: Color = Color::Rgb(0xbd, 0x93, 0xf9);
pub const FG: Color = Color::Rgb(0xf8, 0xf8, 0xf2);
pub const FG2: Color = Color::Rgb(0xd2, 0xd2, 0xdc);
pub const MUTE: Color = Color::Rgb(0x82, 0x8c, 0xb4);
pub const DIM: Color = Color::Rgb(0x4f, 0x53, 0x73);
pub const SELECTED_BG: Color = Color::Rgb(0x1a, 0x1a, 0x1a);
pub const RED: Color = Color::Rgb(0xff, 0x55, 0x55);
pub const GREEN: Color = Color::Rgb(0x50, 0xfa, 0x7b);
pub const YELLOW: Color = Color::Rgb(0xf1, 0xfa, 0x8c);

fn fg(color: Color) -> Style {
    Style::new().fg(color)
}

pub fn status_color(status: &str) -> Color {
    match status {
        "working" => YELLOW,
        "blocked" => RED,
        "done" => GREEN,
        _ => MUTE,
    }
}

fn width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// Cut `text` to `max` display columns, ending in "…" when cut.
pub fn fit(text: &str, max: usize) -> String {
    if width(text) <= max {
        return text.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w + 1 > max {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
    out
}

fn pad(text: &str, columns: usize) -> String {
    let text = fit(text, columns);
    let gap = columns.saturating_sub(width(&text));
    format!("{text}{}", " ".repeat(gap))
}

/// Draw the whole window.
pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    // Edge case 6.9: below 20 × 4 only the escape hatch fits.
    if area.width < 20 || area.height < 4 {
        let line = Line::from(vec![Span::styled("Too small · ⎋", fg(MUTE))]);
        frame
            .buffer_mut()
            .set_line(area.x, area.y, &line, area.width);
        return;
    }
    let margin = if area.width >= 30 { 1 } else { 0 };
    let inner = Rect {
        x: area.x + margin,
        y: area.y,
        width: area.width - 2 * margin,
        height: area.height,
    };
    let show_header = area.height >= 8;
    let narrow = area.width < 40;
    let mut y = inner.y;
    if show_header {
        let header = header_line(app, inner.width as usize);
        frame
            .buffer_mut()
            .set_line(inner.x, y, &header, inner.width);
        y += 1;
    }
    // Filter field.
    let placeholder = match (app.mode, app.drill.is_some()) {
        (Mode::Send, false) => "filter tabs and spaces",
        (Mode::Send, true) => "filter tabs",
        (Mode::Fetch, _) => "filter panes, tabs and spaces",
    };
    let mut spans = vec![Span::styled("› ", fg(ACCENT))];
    if app.filter.is_empty() {
        spans.push(Span::styled(placeholder, fg(DIM)));
    } else {
        spans.push(Span::styled(app.filter.clone(), fg(FG)));
    }
    frame
        .buffer_mut()
        .set_line(inner.x, y, &Line::from(spans), inner.width);
    let cursor_x =
        inner.x + 2 + width(&app.filter).min(inner.width.saturating_sub(3) as usize) as u16;
    frame.set_cursor_position((cursor_x, y));
    y += 1;
    if area.height >= 14 {
        y += 1;
    }

    let footer_y = area.y + area.height - 1;
    let list_height = footer_y.saturating_sub(y);
    let rows = app.rows();
    let selected = app.selected_index(&rows);
    // Keep the selection visible.
    let visible = usize::from(list_height).max(1);
    if let Some(index) = selected {
        if index < app.scroll {
            app.scroll = index;
        } else if index >= app.scroll + visible {
            app.scroll = index + 1 - visible;
        }
    }
    app.scroll = app.scroll.min(rows.len().saturating_sub(visible));
    app.list = ListGeometry {
        top: y,
        height: list_height,
        first: app.scroll,
    };
    let columns = Columns::measure(&rows, inner.width as usize, narrow);
    for (offset, row) in rows.iter().enumerate().skip(app.scroll).take(visible) {
        let line_y = y + (offset - app.scroll) as u16;
        let is_selected = Some(offset) == selected;
        let row_rect = Rect {
            x: area.x,
            y: line_y,
            width: area.width,
            height: 1,
        };
        if is_selected {
            frame
                .buffer_mut()
                .set_style(row_rect, Style::new().bg(SELECTED_BG));
            frame.buffer_mut().set_line(
                area.x,
                line_y,
                &Line::from(Span::styled("▌", fg(ACCENT).bg(SELECTED_BG))),
                1,
            );
        }
        let line = row_line(row, is_selected, &columns, inner.width as usize);
        frame
            .buffer_mut()
            .set_line(inner.x, line_y, &line, inner.width);
        if is_selected {
            frame.buffer_mut().set_style(
                Rect {
                    x: inner.x,
                    width: inner.width,
                    ..row_rect
                },
                Style::new().bg(SELECTED_BG),
            );
        }
    }
    let footer = footer_line(app, inner.width as usize);
    frame
        .buffer_mut()
        .set_line(inner.x, footer_y, &footer, inner.width);
}

fn header_line(app: &App, max: usize) -> Line<'static> {
    let Some(source) = &app.source else {
        return Line::from(Span::styled(crate::plan::PANE_CLOSED, fg(RED)));
    };
    let words = names::pane_words(source, app.commands.get(&source.terminal_id));
    let mut spans: Vec<Span<'static>> = Vec::new();
    match app.mode {
        Mode::Send => {
            spans.push(Span::styled("moving ", fg(MUTE)));
            spans.push(Span::styled(
                words.glyph,
                fg(status_color(&source.agent_status)),
            ));
            spans.push(Span::styled(
                format!(" {}", words.who),
                fg(FG).add_modifier(Modifier::BOLD),
            ));
            if !words.title.is_empty() {
                spans.push(Span::styled(format!("  {}", words.title), fg(FG2)));
            }
            if !words.folder.is_empty() {
                spans.push(Span::styled(format!("  {}", words.folder), fg(MUTE)));
            }
        }
        Mode::Fetch => {
            let tab = app
                .snapshot
                .tab(&source.tab_id)
                .map(names::tab_label)
                .unwrap_or_default();
            let space = app
                .snapshot
                .workspace(&source.workspace_id)
                .map(|s| names::space_label(&app.snapshot, s))
                .unwrap_or_default();
            let you = if words.title.is_empty() {
                words.who
            } else {
                words.title
            };
            spans.push(Span::styled("fetch into ", fg(MUTE)));
            spans.push(Span::styled(tab, fg(FG)));
            spans.push(Span::styled(format!(" · {space}, "), fg(MUTE)));
            spans.push(Span::styled(format!("{} of ", app.split.word()), fg(MUTE)));
            spans.push(Span::styled(you, fg(FG)));
        }
    }
    clip_spans(spans, max)
}

fn clip_spans(spans: Vec<Span<'static>>, max: usize) -> Line<'static> {
    let mut used = 0;
    let mut out = Vec::new();
    for span in spans {
        let w = width(&span.content);
        if used + w <= max {
            used += w;
            out.push(span);
        } else {
            let room = max.saturating_sub(used);
            if room > 0 {
                out.push(Span::styled(fit(&span.content, room), span.style));
            }
            break;
        }
    }
    Line::from(out)
}

/// Column widths shared by every row, so names and details line up.
struct Columns {
    name: usize,
    who: usize,
    narrow: bool,
}

impl Columns {
    fn measure(rows: &[Row], total: usize, narrow: bool) -> Self {
        let name = rows
            .iter()
            .filter(|r| matches!(r.style, RowStyle::Normal | RowStyle::Dim) && r.indent < 4)
            .map(|r| width(&r.name) + usize::from(r.indent))
            .max()
            .unwrap_or(0)
            .min(total * 45 / 100)
            .max(8);
        let who = rows
            .iter()
            .filter(|r| r.indent >= 4)
            .map(|r| width(&r.name))
            .max()
            .unwrap_or(0)
            .min(total * 32 / 100)
            .max(5);
        Self { name, who, narrow }
    }
}

fn row_line(row: &Row, selected: bool, columns: &Columns, total: usize) -> Line<'static> {
    let dim = row.style == RowStyle::Dim;
    let text = if dim { fg(DIM) } else { fg(FG) };
    let soft = if dim { fg(DIM) } else { fg(MUTE) };
    match row.style {
        RowStyle::Section => {
            let mut spans = vec![Span::styled(row.name.clone(), fg(ACCENT))];
            if !row.note.is_empty() {
                spans.push(Span::styled(row.note.clone(), fg(MUTE)));
            }
            return clip_spans(spans, total);
        }
        RowStyle::Message => {
            return clip_spans(vec![Span::styled(row.name.clone(), fg(MUTE))], total)
        }
        RowStyle::Title => {
            let hint = row.hint.clone();
            let room = total.saturating_sub(width(&hint) + 1);
            let name = pad(&row.name, room);
            return Line::from(vec![
                Span::styled(name, fg(FG).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" {hint}"), fg(MUTE)),
            ]);
        }
        RowStyle::Normal | RowStyle::Dim => {}
    }
    let right = if row.indent >= 4 {
        row.detail.clone()
    } else if selected && !row.selected_hint.is_empty() {
        row.selected_hint.clone()
    } else {
        row.hint.clone()
    };
    let right_style = if row.indent >= 4 && row.detail == "you" {
        fg(FG2)
    } else {
        soft
    };
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0;
    let lead = " ".repeat(usize::from(row.indent) + 1);
    used += width(&lead);
    spans.push(Span::raw(lead));
    if row.indent >= 4 {
        // Pane row: status dot, who, title … folder.
        let glyph = if row.glyph.is_empty() {
            " "
        } else {
            row.glyph.as_str()
        };
        let glyph_style = if dim {
            fg(DIM)
        } else {
            fg(status_color(&row.status))
        };
        spans.push(Span::styled(format!("{} ", pad(glyph, 1)), glyph_style));
        used += 2;
        let right_w = if columns.narrow { 0 } else { width(&right) };
        let room = total.saturating_sub(used + right_w + 1);
        let who_w = columns.who.min(room);
        spans.push(Span::styled(pad(&row.name, who_w), text));
        used += who_w;
        if !row.title.is_empty() && room > who_w + 2 {
            let title = fit(&row.title, room - who_w - 2);
            used += 2 + width(&title);
            spans.push(Span::styled(
                format!("  {title}"),
                if dim { fg(DIM) } else { fg(FG2) },
            ));
        }
        if right_w > 0 {
            let gap = total.saturating_sub(used + right_w);
            spans.push(Span::raw(" ".repeat(gap)));
            spans.push(Span::styled(right, right_style));
        }
        return Line::from(spans);
    }
    // Send rows and Fetch tab rows: glyph, name, detail … hint.
    if row.indent == 0 {
        let glyph = if row.glyph.is_empty() {
            "  "
        } else {
            row.glyph.as_str()
        };
        spans.push(Span::styled(
            format!("{} ", pad(glyph, 2)),
            if dim { fg(DIM) } else { fg(ACCENT) },
        ));
        used += 3;
    }
    let hint_w = width(&right);
    let room = total.saturating_sub(used + hint_w + 1);
    // Rows share the name column; a longer name may use room its own
    // detail does not need.
    let detail_need = if row.detail.is_empty() {
        0
    } else {
        width(&row.detail) + 2
    };
    let name_w = columns
        .name
        .saturating_sub(usize::from(row.indent))
        .max(width(&row.name).min(room.saturating_sub(detail_need)))
        .min(room);
    let name = if row.detail.is_empty() || columns.narrow {
        fit(&row.name, room)
    } else {
        pad(&row.name, name_w)
    };
    used += width(&name);
    spans.push(Span::styled(name, text));
    if !row.detail.is_empty() && !columns.narrow && room > name_w + 2 {
        let detail = fit(&row.detail, room - name_w - 2);
        used += 2 + width(&detail);
        spans.push(Span::styled(format!("  {detail}"), soft));
    }
    if hint_w > 0 {
        let gap = total.saturating_sub(used + hint_w);
        spans.push(Span::raw(" ".repeat(gap)));
        spans.push(Span::styled(right, soft));
    }
    Line::from(spans)
}

fn footer_line(app: &App, total: usize) -> Line<'static> {
    let lands = vec![
        Span::styled("lands: ", fg(MUTE)),
        Span::styled(app.split.word(), fg(FG2)),
    ];
    let lands_w = 7 + app.split.word().len();
    if let Some(message) = &app.message {
        let color = if message.error { RED } else { FG2 };
        let room = total.saturating_sub(lands_w + 2);
        let text = fit(&message.text, room);
        let gap = total.saturating_sub(width(&text) + lands_w);
        let mut spans = vec![Span::styled(text, fg(color)), Span::raw(" ".repeat(gap))];
        spans.extend(lands);
        return Line::from(spans);
    }
    let mut keys: Vec<(&str, &str)> = match (app.mode, app.drill.is_some()) {
        (Mode::Send, false) => vec![
            ("⏎", "send"),
            ("→", "open space"),
            ("⇥", "right/below"),
            ("⎋", "close"),
        ],
        (Mode::Send, true) => vec![
            ("⏎", "send"),
            ("←", "back"),
            ("⇥", "right/below"),
            ("⎋", "close"),
        ],
        (Mode::Fetch, _) => vec![
            ("⏎", "fetch pane or tab"),
            ("⇥", "right/below"),
            ("⌃⌥S", "send"),
            ("⎋", "close"),
        ],
    };
    let measure = |keys: &[(&str, &str)]| -> usize {
        keys.iter()
            .map(|(k, w)| width(k) + 1 + width(w))
            .sum::<usize>()
            + 3 * keys.len().saturating_sub(1)
    };
    // Drop middle hints until the footer fits; ⏎ and ⎋ always stay.
    while keys.len() > 2 && measure(&keys) + lands_w + 2 > total {
        keys.remove(keys.len() - 2);
    }
    let with_lands = measure(&keys) + lands_w + 2 <= total;
    let mut spans = Vec::new();
    for (index, (key, word)) in keys.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("   "));
        }
        spans.push(Span::styled(key.to_string(), fg(FG)));
        spans.push(Span::styled(format!(" {word}"), fg(MUTE)));
    }
    if with_lands {
        let gap = total - measure(&keys) - lands_w;
        spans.push(Span::raw(" ".repeat(gap)));
        spans.extend(lands);
    }
    clip_spans(spans, total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api;
    use crate::model::Command;
    use crate::sim::{Sim, SAMPLE_FIXTURE};
    use crate::ui::app::Input;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn app(mode: Mode) -> (Sim, App) {
        let sim = Sim::from_json(SAMPLE_FIXTURE).unwrap();
        let snapshot = api::snapshot(&sim).unwrap();
        let source = snapshot.pane(&sim.pane_id("portfolio")).cloned();
        let mut app = App::new(mode, snapshot, source);
        app.commands.insert(
            sim.terminal("scraper"),
            Command {
                display: "python3 scrape_docs.py".into(),
                program: "python3".into(),
                is_shell: false,
            },
        );
        (sim, app)
    }

    fn draw(app: &mut App, w: u16, h: u16) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| render(f, app)).unwrap();
        terminal
    }

    fn lines(terminal: &Terminal<TestBackend>) -> Vec<String> {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                let mut line = String::new();
                let mut x = 0;
                while x < buffer.area.width {
                    let symbol = buffer[(x, y)].symbol();
                    line.push_str(symbol);
                    // A wide glyph owns the next cell too.
                    x += width(symbol).max(1) as u16;
                }
                line.trim_end().to_string()
            })
            .collect()
    }

    fn find(lines: &[String], needle: &str) -> usize {
        lines
            .iter()
            .position(|l| l.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} not in\n{}", lines.join("\n")))
    }

    #[test]
    fn send_window_renders_the_design() {
        let (_sim, mut app) = app(Mode::Send);
        let terminal = draw(&mut app, 72, 16);
        let text = lines(&terminal);
        let all = text.join("\n");
        assert!(
            text[0].contains("moving ○ claude  portfolio copy  acme-web"),
            "{all}"
        );
        assert!(text[1].contains("› filter tabs and spaces"), "{all}");
        let new = find(&text, " new");
        assert!(
            text[new + 1].contains("＋ New tab here")
                && text[new + 1].contains("named claude, next to this tab"),
            "{all}"
        );
        assert!(
            text[new + 1].trim_end().ends_with('⏎'),
            "selected row shows ⏎: {all}"
        );
        assert!(text[find(&text, "this space · Studio")].contains("this space · Studio"));
        let here = find(&text, "Landing_Page_Copy");
        assert!(text[here].trim_end().ends_with("here"));
        let other = find(&text, "Payments_Service_Rewrite");
        assert!(text[other].trim_end().ends_with('→'));
        let footer = text.last().unwrap();
        assert!(
            footer.contains("⏎ send")
                && footer.contains("→ open space")
                && footer.contains("⇥ right/below")
                && footer.contains("⎋ close"),
            "{footer}"
        );
        assert!(footer.ends_with("lands: right"));
    }

    /// The selected row is drawn on #1a1a1a with a ▌ bar, not reverse video;
    /// section headers are accent; the "here" tab is dim.
    #[test]
    fn selected_row_style_and_section_colours() {
        let (_sim, mut app) = app(Mode::Send);
        let terminal = draw(&mut app, 72, 16);
        let text = lines(&terminal);
        let buffer = terminal.backend().buffer();
        let row = find(&text, "New tab here") as u16;
        let bar = &buffer[(0, row)];
        assert_eq!(bar.symbol(), "▌");
        assert_eq!(bar.fg, ACCENT);
        assert_eq!(buffer[(30, row)].bg, SELECTED_BG);
        assert!(!buffer[(10, row)].modifier.contains(Modifier::REVERSED));
        let section = find(&text, " new") as u16;
        assert_eq!(buffer[(1, section)].fg, ACCENT);
        let here = find(&text, "Landing_Page_Copy") as u16;
        assert_eq!(buffer[(5, here)].fg, DIM);
        let unselected = find(&text, "API refactor") as u16;
        assert_ne!(buffer[(5, unselected)].bg, SELECTED_BG);
    }

    #[test]
    fn send_drilled_view_renders_back_and_new_tab_in_space() {
        let (_sim, mut app) = app(Mode::Send);
        app.handle(Input::End);
        app.handle(Input::Right);
        let text = lines(&draw(&mut app, 72, 16));
        let all = text.join("\n");
        assert!(text[1].contains("› filter tabs"), "{all}");
        assert!(
            text[find(&text, "Payments_Service_Rewrite")]
                .trim_end()
                .ends_with("← back"),
            "{all}"
        );
        assert!(
            all.contains("＋ New tab in Payments_Service_Rewrite"),
            "{all}"
        );
        assert!(text[find(&text, "deploying")].contains("1 pane"));
        assert!(text.last().unwrap().contains("← back"));
    }

    #[test]
    fn fetch_window_renders_every_space_by_name() {
        let (_sim, mut app) = app(Mode::Fetch);
        let text = lines(&draw(&mut app, 76, 20));
        let all = text.join("\n");
        assert!(
            text[0].contains("fetch into Landing_Page_Copy · Studio, right of portfolio copy"),
            "{all}"
        );
        assert!(text[1].contains("› filter panes, tabs and spaces"));
        let python = find(&text, "python3 scrape_docs.py");
        assert!(text[python].trim_end().ends_with("acme-web"), "{all}");
        assert!(
            text[find(&text, "portfolio copy  ")]
                .trim_end()
                .ends_with("you")
                || all.contains("you")
        );
        // The first selectable row is a tab: it shows the whole-tab hint.
        assert!(
            text[find(&text, "API refactor")]
                .trim_end()
                .ends_with("⏎ whole tab"),
            "{all}"
        );
        assert!(!all.contains("w1:p"), "raw ids never shown: {all}");
        let footer = text.last().unwrap();
        assert!(
            footer.contains("⏎ fetch pane or tab") && footer.contains("⌃⌥S send"),
            "{footer}"
        );
    }

    #[test]
    fn toggle_changes_header_and_footer_to_below() {
        let (_sim, mut app) = app(Mode::Fetch);
        app.handle(Input::Toggle);
        let text = lines(&draw(&mut app, 76, 20));
        assert!(
            text[0].contains("below of") || text[0].contains(", below"),
            "{}",
            text[0]
        );
        assert!(text.last().unwrap().ends_with("lands: below"));
    }

    /// Edge case 7.5: errors show inline in the footer and the window stays.
    #[test]
    fn edge_7_5_inline_error_in_footer() {
        let (_sim, mut app) = app(Mode::Send);
        app.show_error("That pane closed");
        let text = lines(&draw(&mut app, 72, 16));
        let footer = text.last().unwrap();
        assert!(footer.starts_with(" That pane closed"), "{footer}");
    }

    /// Edge case 6.9: small terminals drop columns, then the header, then
    /// show only "Too small · ⎋".
    #[test]
    fn edge_6_9_small_terminals() {
        let (_sim, mut app) = app(Mode::Send);
        let text = lines(&draw(&mut app, 45, 10));
        assert!(
            text[0].starts_with(" moving"),
            "80×24 → ~45×10 keeps the header"
        );
        assert!(text.iter().any(|l| l.contains("New tab here")));
        let text = lines(&draw(&mut app, 36, 7));
        assert!(
            text[0].contains("› filter"),
            "below 8 rows the header goes: {text:?}"
        );
        assert!(
            !text.iter().any(|l| l.contains("named claude")),
            "below 40 columns details go: {text:?}"
        );
        let text = lines(&draw(&mut app, 18, 3));
        assert_eq!(text[0], "Too small · ⎋");
    }

    /// Edge case 6.14: a resize redraws and keeps filter, selection, split.
    #[test]
    fn edge_6_14_resize_keeps_state() {
        let (_sim, mut app) = app(Mode::Send);
        app.handle(Input::Char('j'));
        app.handle(Input::Toggle);
        let before = app.selected.clone();
        let text = lines(&draw(&mut app, 60, 12));
        assert!(text[1].contains("› j"));
        let text = lines(&draw(&mut app, 90, 30));
        assert!(text[1].contains("› j"));
        assert_eq!(app.selected, before);
        assert!(text.last().unwrap().ends_with("lands: below"));
    }
}
