//! Window state and key handling, independent of the terminal.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use crate::model::{Command, PaneInfo, Snapshot, SplitDir};
use crate::ui::rows::{self, Row, RowAction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Send,
    Fetch,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Send => "send",
            Self::Fetch => "fetch",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "send" => Some(Self::Send),
            "fetch" => Some(Self::Fetch),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Char(char),
    Backspace,
    ClearFilter,
    DeleteWord,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Enter,
    Left,
    Right,
    Toggle,
    Close,
    SwitchSend,
    SwitchFetch,
    Click { row: u16, double: bool },
    ScrollUp,
    ScrollDown,
    Ignore,
}

/// Map one key to an input (edge cases 4.10, 6.12, 6.13, 6.17).
///
/// herdr delivers ⌃⌥S to a popup as ESC + ^S, which crossterm reports as
/// `s` with CONTROL|ALT. A lone ESC arrives as Esc; ESC followed by another
/// byte in the same read is a chord, never a close.
pub fn map_key(key: KeyEvent) -> Input {
    if key.kind == KeyEventKind::Release {
        return Input::Ignore;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    if ctrl && alt {
        return match key.code {
            KeyCode::Char('s') | KeyCode::Char('S') => Input::SwitchSend,
            KeyCode::Char('f') | KeyCode::Char('F') => Input::SwitchFetch,
            _ => Input::Ignore,
        };
    }
    if alt {
        return Input::Ignore;
    }
    if ctrl {
        return match key.code {
            KeyCode::Char('c') => Input::Close,
            KeyCode::Char('u') => Input::ClearFilter,
            KeyCode::Char('w') => Input::DeleteWord,
            KeyCode::Char('h') => Input::Backspace,
            KeyCode::Char('p') => Input::Up,
            KeyCode::Char('n') => Input::Down,
            KeyCode::Char('j') | KeyCode::Char('m') => Input::Enter,
            KeyCode::Backspace => Input::DeleteWord,
            _ => Input::Ignore,
        };
    }
    match key.code {
        KeyCode::Esc => Input::Close,
        KeyCode::Enter => Input::Enter,
        KeyCode::Tab | KeyCode::BackTab => Input::Toggle,
        KeyCode::Backspace => Input::Backspace,
        KeyCode::Up => Input::Up,
        KeyCode::Down => Input::Down,
        KeyCode::Left => Input::Left,
        KeyCode::Right => Input::Right,
        KeyCode::PageUp => Input::PageUp,
        KeyCode::PageDown => Input::PageDown,
        KeyCode::Home => Input::Home,
        KeyCode::End => Input::End,
        KeyCode::Char(c) if !c.is_control() => Input::Char(c),
        _ => Input::Ignore,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    None,
    Close,
    Execute(RowAction),
    Switch(Mode),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub text: String,
    pub error: bool,
}

/// Where the list was drawn, for mouse hit-testing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ListGeometry {
    pub top: u16,
    pub height: u16,
    pub first: usize,
}

pub struct App {
    pub mode: Mode,
    pub snapshot: Snapshot,
    /// Send: the moving pane. Fetch: your pane, where things land.
    pub source: Option<PaneInfo>,
    pub filter: String,
    pub split: SplitDir,
    pub drill: Option<String>,
    pub selected: Option<String>,
    selected_hint: usize,
    pub scroll: usize,
    pub message: Option<Message>,
    pub commands: HashMap<String, Command>,
    pub list: ListGeometry,
    last_click: Option<(Instant, usize)>,
}

impl App {
    pub fn new(mode: Mode, snapshot: Snapshot, source: Option<PaneInfo>) -> Self {
        let mut app = Self {
            mode,
            snapshot,
            source,
            filter: String::new(),
            split: SplitDir::Right,
            drill: None,
            selected: None,
            selected_hint: 0,
            scroll: 0,
            message: None,
            commands: HashMap::new(),
            list: ListGeometry::default(),
            last_click: None,
        };
        app.ensure_selection();
        app
    }

    /// Start over in the other window (preview mode switches in place).
    pub fn switch(&mut self, mode: Mode) {
        self.mode = mode;
        self.filter.clear();
        self.drill = None;
        self.selected = None;
        self.scroll = 0;
        self.message = None;
        self.ensure_selection();
    }

    pub fn rows(&self) -> Vec<Row> {
        let Some(source) = &self.source else {
            return vec![Row::message(crate::plan::PANE_CLOSED)];
        };
        match self.mode {
            Mode::Send => rows::send_rows(
                &self.snapshot,
                source,
                &self.commands,
                &self.filter,
                self.drill.as_deref(),
            ),
            Mode::Fetch => rows::fetch_rows(&self.snapshot, source, &self.commands, &self.filter),
        }
    }

    pub fn selected_index(&self, rows: &[Row]) -> Option<usize> {
        self.selected
            .as_ref()
            .and_then(|key| rows.iter().position(|r| r.selectable && &r.key == key))
    }

    /// Keep the selection on the same row, or the nearest selectable one.
    fn ensure_selection(&mut self) {
        let rows = self.rows();
        if self.selected_index(&rows).is_some() {
            return;
        }
        let hint = self.selected_hint.min(rows.len().saturating_sub(1));
        let pick = rows[hint..]
            .iter()
            .position(|r| r.selectable)
            .map(|i| i + hint)
            .or_else(|| rows[..hint].iter().rposition(|r| r.selectable));
        self.selected = pick.map(|i| rows[i].key.clone());
    }

    fn select_index(&mut self, rows: &[Row], index: usize) {
        if let Some(row) = rows.get(index).filter(|r| r.selectable) {
            self.selected = Some(row.key.clone());
            self.selected_hint = index;
        }
    }

    fn step(&mut self, delta: isize) {
        let rows = self.rows();
        let selectable: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].selectable).collect();
        if selectable.is_empty() {
            self.selected = None;
            return;
        }
        let current = self
            .selected_index(&rows)
            .and_then(|i| selectable.iter().position(|&s| s == i))
            .unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, selectable.len() as isize - 1) as usize;
        self.select_index(&rows, selectable[next]);
    }

    /// New herdr state arrived (edge case 6.15).
    pub fn set_snapshot(&mut self, snapshot: Snapshot) {
        if let Some(source) = &self.source {
            let terminal = source.terminal_id.clone();
            self.source = snapshot.pane_by_terminal(&terminal).cloned();
        }
        if let Some(drill) = &self.drill {
            if snapshot.workspace(drill).is_none() {
                self.drill = None;
            }
        }
        self.snapshot = snapshot;
        let rows = self.rows();
        if let Some(index) = self.selected_index(&rows) {
            self.selected_hint = index;
        }
        self.ensure_selection();
    }

    fn filter_changed(&mut self) {
        self.selected = None;
        self.selected_hint = 0;
        self.scroll = 0;
        self.message = None;
        // Land on the first row the words match itself: typing a pane's
        // command selects that pane, not the tab row listed above it.
        let rows = self.rows();
        if let Some(index) = rows.iter().position(|r| r.selectable && r.matched) {
            self.select_index(&rows, index);
        }
        self.ensure_selection();
    }

    fn activate(&mut self, index: Option<usize>) -> Effect {
        let rows = self.rows();
        let Some(row) = index.and_then(|i| rows.get(i)).filter(|r| r.selectable) else {
            return Effect::None;
        };
        match &row.action {
            RowAction::OpenSpace(workspace_id) => {
                self.open_space(workspace_id.clone());
                Effect::None
            }
            RowAction::Back => {
                self.back();
                Effect::None
            }
            RowAction::None => Effect::None,
            action => Effect::Execute(action.clone()),
        }
    }

    fn open_space(&mut self, workspace_id: String) {
        self.drill = Some(workspace_id);
        self.filter.clear();
        self.filter_changed();
    }

    fn back(&mut self) {
        if let Some(workspace_id) = self.drill.take() {
            self.filter.clear();
            self.message = None;
            self.scroll = 0;
            self.selected = Some(format!("space:{workspace_id}"));
            let rows = self.rows();
            if let Some(index) = self.selected_index(&rows) {
                self.selected_hint = index;
            }
            self.ensure_selection();
        }
    }

    pub fn show_error(&mut self, text: impl Into<String>) {
        self.message = Some(Message {
            text: text.into(),
            error: true,
        });
    }

    pub fn show_info(&mut self, text: impl Into<String>) {
        self.message = Some(Message {
            text: text.into(),
            error: false,
        });
    }

    /// Translate a mouse event into an input using the drawn list geometry.
    pub fn mouse(&mut self, event: MouseEvent) -> Input {
        match event.kind {
            MouseEventKind::ScrollUp => Input::ScrollUp,
            MouseEventKind::ScrollDown => Input::ScrollDown,
            MouseEventKind::Down(MouseButton::Left) => {
                let ListGeometry { top, height, first } = self.list;
                if event.row < top || event.row >= top + height {
                    return Input::Ignore;
                }
                let index = first + usize::from(event.row - top);
                let now = Instant::now();
                let double = matches!(self.last_click, Some((at, i)) if i == index && now.duration_since(at) < Duration::from_millis(400));
                self.last_click = Some((now, index));
                Input::Click {
                    row: index as u16,
                    double,
                }
            }
            _ => Input::Ignore,
        }
    }

    pub fn handle(&mut self, input: Input) -> Effect {
        match input {
            Input::Close => return Effect::Close,
            Input::SwitchSend if self.mode == Mode::Fetch => return Effect::Switch(Mode::Send),
            Input::SwitchFetch if self.mode == Mode::Send => return Effect::Switch(Mode::Fetch),
            Input::SwitchSend | Input::SwitchFetch | Input::Ignore => {}
            Input::Char(c) => {
                self.filter.push(c);
                self.filter_changed();
            }
            Input::Backspace => {
                if self.filter.pop().is_some() {
                    self.filter_changed();
                }
            }
            Input::ClearFilter => {
                self.filter.clear();
                self.filter_changed();
            }
            Input::DeleteWord => {
                let trimmed = self.filter.trim_end().to_string();
                let cut = trimmed.rfind(' ').map(|i| i + 1).unwrap_or(0);
                self.filter.truncate(cut);
                self.filter_changed();
            }
            Input::Up | Input::ScrollUp => self.step(-1),
            Input::Down | Input::ScrollDown => self.step(1),
            Input::PageUp => self.step(-(self.list.height.max(2) as isize - 1)),
            Input::PageDown => self.step(self.list.height.max(2) as isize - 1),
            Input::Home => self.step(isize::MIN / 2),
            Input::End => self.step(isize::MAX / 2),
            Input::Toggle => self.split = self.split.toggled(),
            Input::Enter => {
                let rows = self.rows();
                return self.activate(self.selected_index(&rows));
            }
            Input::Right => {
                let rows = self.rows();
                if let Some(index) = self.selected_index(&rows) {
                    if let RowAction::OpenSpace(id) = &rows[index].action {
                        self.open_space(id.clone());
                    }
                }
            }
            Input::Left => self.back(),
            Input::Click { row, double } => {
                let rows = self.rows();
                let index = usize::from(row);
                self.select_index(&rows, index);
                if double {
                    return self.activate(Some(index));
                }
            }
        }
        Effect::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api;
    use crate::plan::{FetchTarget, SendTarget};
    use crate::sim::{Sim, SAMPLE_FIXTURE};

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn app(mode: Mode) -> (Sim, App) {
        let sim = Sim::from_json(SAMPLE_FIXTURE).unwrap();
        let snapshot = api::snapshot(&sim).unwrap();
        let source = snapshot.pane(&sim.pane_id("portfolio")).cloned();
        (sim, App::new(mode, snapshot, source))
    }

    /// Edge cases 4.10 and 6.12: ⌃⌥S/⌃⌥F switch windows, other ⌃⌥ and ⌥
    /// keys do nothing, Esc closes.
    #[test]
    fn edge_4_10_and_6_12_chords() {
        let ctrl_alt = KeyModifiers::CONTROL | KeyModifiers::ALT;
        assert_eq!(
            map_key(key(KeyCode::Char('s'), ctrl_alt)),
            Input::SwitchSend
        );
        assert_eq!(
            map_key(key(KeyCode::Char('f'), ctrl_alt)),
            Input::SwitchFetch
        );
        for code in [
            KeyCode::Char('t'),
            KeyCode::Char('n'),
            KeyCode::Char('z'),
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Char('m'),
        ] {
            assert_eq!(map_key(key(code, ctrl_alt)), Input::Ignore, "{code:?}");
        }
        assert_eq!(
            map_key(key(KeyCode::Char('x'), KeyModifiers::ALT)),
            Input::Ignore
        );
        assert_eq!(map_key(key(KeyCode::Esc, KeyModifiers::NONE)), Input::Close);
    }

    /// Edge case 4.16: ⌃⌥M belongs to Codemap; paneMorph ignores it.
    #[test]
    fn edge_4_16_ctrl_alt_m_is_ignored() {
        let key = KeyEvent::new(
            KeyCode::Char('m'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        );
        assert_eq!(map_key(key), Input::Ignore);
        let (_sim, mut app) = app(Mode::Send);
        assert_eq!(app.handle(map_key(key)), Effect::None);
    }

    /// Edge case 6.13: ⌃C closes like Esc.
    #[test]
    fn edge_6_13_ctrl_c_closes() {
        assert_eq!(
            map_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Input::Close
        );
    }

    /// Edge case 4.10: the window's own key is ignored; the other switches.
    #[test]
    fn edge_4_10_own_key_ignored_other_switches() {
        let (_sim, mut app) = app(Mode::Send);
        assert_eq!(app.handle(Input::SwitchSend), Effect::None);
        assert_eq!(app.handle(Input::SwitchFetch), Effect::Switch(Mode::Fetch));
        let (_sim, mut app) = self::app(Mode::Fetch);
        assert_eq!(app.handle(Input::SwitchFetch), Effect::None);
        assert_eq!(app.handle(Input::SwitchSend), Effect::Switch(Mode::Send));
    }

    /// Edge case 6.17: letters filter, Backspace, ⌃U and ⌃W edit it, and
    /// ← → never move a filter cursor.
    #[test]
    fn edge_6_17_filter_keys() {
        assert_eq!(
            map_key(key(KeyCode::Char('u'), KeyModifiers::CONTROL)),
            Input::ClearFilter
        );
        assert_eq!(
            map_key(key(KeyCode::Char('w'), KeyModifiers::CONTROL)),
            Input::DeleteWord
        );
        let (_sim, mut app) = app(Mode::Send);
        for c in "load te".chars() {
            app.handle(Input::Char(c));
        }
        app.handle(Input::DeleteWord);
        assert_eq!(app.filter, "load ");
        app.handle(Input::Backspace);
        assert_eq!(app.filter, "load");
        app.handle(Input::Left);
        assert_eq!(app.filter, "load");
        app.handle(Input::ClearFilter);
        assert_eq!(app.filter, "");
    }

    #[test]
    fn send_selection_starts_on_new_tab_and_skips_disabled_rows() {
        let (_sim, mut app) = app(Mode::Send);
        assert_eq!(app.selected.as_deref(), Some("new-tab"));
        app.handle(Input::Down); // New space
        app.handle(Input::Down); // skips "here" to API refactor
        let rows = app.rows();
        assert_eq!(
            rows[app.selected_index(&rows).unwrap()].name,
            "API refactor"
        );
    }

    #[test]
    fn enter_sends_to_the_selected_tab() {
        let (sim, mut app) = app(Mode::Send);
        for c in "load".chars() {
            app.handle(Input::Char(c));
        }
        assert_eq!(
            app.handle(Input::Enter),
            Effect::Execute(RowAction::Send(SendTarget::Tab(sim.tab_id_of("scraper"))))
        );
    }

    /// Typing a pane's words selects the pane, not its tab; typing a tab's
    /// name selects the tab row.
    #[test]
    fn filter_selects_the_first_row_that_matches_itself() {
        let (sim, mut app) = app(Mode::Fetch);
        app.commands.insert(
            sim.terminal("scraper"),
            Command {
                display: "python3 scrape_docs.py".into(),
                program: "python3".into(),
                is_shell: false,
            },
        );
        for c in "scrape".chars() {
            app.handle(Input::Char(c));
        }
        assert_eq!(
            app.handle(Input::Enter),
            Effect::Execute(RowAction::FetchPane(sim.pane_id("scraper")))
        );
        app.handle(Input::ClearFilter);
        for c in "load".chars() {
            app.handle(Input::Char(c));
        }
        assert_eq!(
            app.handle(Input::Enter),
            Effect::Execute(RowAction::FetchTab(sim.tab_id_of("scraper")))
        );
    }

    /// Right opens a space, Left goes back to it, Enter also opens.
    #[test]
    fn drill_in_and_back() {
        let (_sim, mut app) = app(Mode::Send);
        app.handle(Input::End);
        app.handle(Input::Right);
        assert_eq!(app.drill.as_deref(), Some("w2"));
        assert_eq!(app.selected.as_deref(), Some("new-tab-in:w2"));
        app.handle(Input::Left);
        assert_eq!(app.drill, None);
        assert_eq!(app.selected.as_deref(), Some("space:w2"));
        assert_eq!(app.handle(Input::Enter), Effect::None);
        assert_eq!(app.drill.as_deref(), Some("w2"));
    }

    /// Edge case 1.14: ⇥ toggles right/below and starts at right.
    #[test]
    fn edge_1_14_tab_toggles_split() {
        let (_sim, mut app) = app(Mode::Fetch);
        assert_eq!(app.split, SplitDir::Right);
        app.handle(Input::Toggle);
        assert_eq!(app.split, SplitDir::Down);
    }

    #[test]
    fn fetch_enter_on_tab_row_fetches_whole_tab_and_pane_row_fetches_pane() {
        let (sim, mut app) = app(Mode::Fetch);
        // First selectable row is "API refactor" (the current tab is dimmed).
        assert_eq!(
            app.handle(Input::Enter),
            Effect::Execute(RowAction::FetchTab(sim.tab_id_of("zsh")))
        );
        app.handle(Input::Down);
        assert_eq!(
            app.handle(Input::Enter),
            Effect::Execute(RowAction::FetchPane(sim.pane_id("zsh")))
        );
        let _ = FetchTarget::Pane(String::new());
    }

    /// Edge case 6.15: a refresh keeps the selection on the same row, or
    /// the nearest one when it vanished.
    #[test]
    fn edge_6_15_selection_survives_refresh() {
        let (sim, mut app) = app(Mode::Fetch);
        app.handle(Input::Down);
        app.handle(Input::Down);
        app.handle(Input::Down); // "python3" pane row (scraper)
        let key = app.selected.clone().unwrap();
        assert!(key.starts_with("pane:"));
        let fresh = api::snapshot(&sim).unwrap();
        app.set_snapshot(fresh);
        assert_eq!(app.selected.as_ref(), Some(&key));
        sim.close_pane("scraper");
        app.set_snapshot(api::snapshot(&sim).unwrap());
        assert!(app.selected.is_some());
        assert_ne!(app.selected.as_ref(), Some(&key));
    }

    /// Edge case 6.18: a click selects, a double-click acts.
    #[test]
    fn edge_6_18_mouse_click_and_double_click() {
        let (sim, mut app) = app(Mode::Send);
        app.list = ListGeometry {
            top: 3,
            height: 10,
            first: 0,
        };
        let click = |app: &mut App| {
            app.mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 5,
                row: 3 + 5,
                modifiers: KeyModifiers::NONE,
            })
        };
        let first = click(&mut app);
        assert_eq!(
            first,
            Input::Click {
                row: 5,
                double: false
            }
        );
        assert_eq!(app.handle(first), Effect::None);
        let second = click(&mut app);
        assert_eq!(
            second,
            Input::Click {
                row: 5,
                double: true
            }
        );
        assert_eq!(
            app.handle(second),
            Effect::Execute(RowAction::Send(SendTarget::Tab(sim.tab_id_of("zsh"))))
        );
    }
}
