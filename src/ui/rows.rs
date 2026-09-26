//! The rows each window lists, built from one snapshot. Pure and testable.

use std::collections::HashMap;

use crate::model::{Command, PaneInfo, Snapshot};
use crate::names;
use crate::plan::{self, SendTarget};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowAction {
    None,
    Send(SendTarget),
    OpenSpace(String),
    Back,
    FetchPane(String),
    FetchTab(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowStyle {
    /// Lowercase accent section header.
    Section,
    /// A header in plain text (the opened space, a space in Fetch).
    Title,
    Normal,
    Dim,
    /// A full-width message such as "Nothing to fetch".
    Message,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Stable identity used to keep the selection across refreshes (6.15).
    pub key: String,
    pub style: RowStyle,
    pub indent: u16,
    pub glyph: String,
    /// Agent status for pane rows, to colour the glyph.
    pub status: String,
    pub name: String,
    /// Muted text after the name (a section header's "· Studio").
    pub note: String,
    pub detail: String,
    /// Right-hand hint; the selected row shows its own hint instead.
    pub hint: String,
    pub selected_hint: String,
    pub selectable: bool,
    pub action: RowAction,
    /// Pane rows split `name` into who and title columns.
    pub title: String,
}

impl Row {
    fn section(key: &str, name: &str, note: &str) -> Self {
        Self {
            key: format!("section:{key}"),
            style: RowStyle::Section,
            indent: 0,
            glyph: String::new(),
            status: String::new(),
            name: name.to_string(),
            note: note.to_string(),
            detail: String::new(),
            hint: String::new(),
            selected_hint: String::new(),
            selectable: false,
            action: RowAction::None,
            title: String::new(),
        }
    }

    fn item(key: String, name: String, action: RowAction) -> Self {
        Self {
            key,
            style: RowStyle::Normal,
            indent: 0,
            glyph: String::new(),
            status: String::new(),
            name,
            note: String::new(),
            detail: String::new(),
            hint: String::new(),
            selected_hint: "⏎".into(),
            selectable: true,
            action,
            title: String::new(),
        }
    }

    fn disabled(mut self, why: &str) -> Self {
        self.style = RowStyle::Dim;
        self.selectable = false;
        self.detail = why.to_string();
        self.selected_hint.clear();
        self
    }

    pub fn message(text: &str) -> Self {
        let mut row = Self::section("message", text, "");
        row.style = RowStyle::Message;
        row
    }
}

/// Every word of the filter must appear in the haystack, ignoring case
/// (edge case 2.11). Raw ids are never part of a haystack.
pub fn matches(filter: &str, haystack: &str) -> bool {
    let haystack = haystack.to_lowercase();
    filter
        .to_lowercase()
        .split_whitespace()
        .all(|word| haystack.contains(word))
}

fn pane_count(n: usize) -> String {
    format!("{n} pane{}", if n == 1 { "" } else { "s" })
}

/// Push `section` followed by `items`, or nothing when every item is
/// filtered out.
fn push_section(rows: &mut Vec<Row>, section: Row, items: Vec<Row>) {
    if !items.is_empty() {
        rows.push(section);
        rows.extend(items);
    }
}

/// Rows of the Send window (design: "Send", edge cases 1.1–1.9, 6.16).
pub fn send_rows(
    snapshot: &Snapshot,
    source: &PaneInfo,
    commands: &HashMap<String, Command>,
    filter: &str,
    drill: Option<&str>,
) -> Vec<Row> {
    let command = commands.get(&source.terminal_id);
    let tab_name = names::tab_base_name(source, command);
    let mut rows = Vec::new();

    if let Some(workspace_id) = drill {
        let Some(space) = snapshot.workspace(workspace_id) else {
            rows.push(Row::message(plan::SPACE_CLOSED));
            return rows;
        };
        let label = names::space_label(snapshot, space);
        let mut title = Row::section("drill", &label, "");
        title.style = RowStyle::Title;
        title.hint = "← back".into();
        rows.push(title);
        let warning = plan::closes_space_warning(snapshot, source, workspace_id);
        let mut new_items = Vec::new();
        let name = names::unique_tab_name(snapshot, workspace_id, &tab_name);
        let mut new_tab = Row::item(
            format!("new-tab-in:{workspace_id}"),
            format!("New tab in {label}"),
            RowAction::Send(SendTarget::NewTabIn(workspace_id.to_string())),
        );
        new_tab.glyph = "＋".into();
        new_tab.detail = match &warning {
            Some(w) => format!("named {name}, {w}"),
            None => format!("named {name}"),
        };
        if matches(filter, &format!("new tab {label} {name}")) {
            new_items.push(new_tab);
        }
        push_section(&mut rows, Row::section("new", "new", ""), new_items);
        let mut tabs = Vec::new();
        for tab in snapshot.tabs_in(workspace_id) {
            let tab_label = names::tab_label(tab);
            if !matches(filter, &tab_label) {
                continue;
            }
            let mut row = Row::item(
                format!("tab:{}", tab.tab_id),
                tab_label,
                RowAction::Send(SendTarget::Tab(tab.tab_id.clone())),
            );
            row.detail = match &warning {
                Some(w) => format!("{}, {w}", pane_count(tab.pane_count)),
                None => pane_count(tab.pane_count),
            };
            tabs.push(row);
        }
        push_section(&mut rows, Row::section("tabs", "tabs", ""), tabs);
        return rows;
    }

    // Make room first: new tab here, new space.
    let mut new_items = Vec::new();
    let here_name = names::unique_tab_name(snapshot, &source.workspace_id, &tab_name);
    let mut new_tab = Row::item(
        "new-tab".into(),
        "New tab here".into(),
        RowAction::Send(SendTarget::NewTabHere),
    );
    new_tab.glyph = "＋".into();
    new_tab.detail = format!("named {here_name}, next to this tab");
    if plan::alone_in_tab(snapshot, source) {
        new_tab = new_tab.disabled("already alone in this tab");
        new_tab.glyph = "＋".into();
    }
    if matches(filter, &format!("new tab here {here_name}")) {
        new_items.push(new_tab);
    }
    let space_name = names::unique_space_name(snapshot, &names::space_base_name(source, command));
    let mut new_space = Row::item(
        "new-space".into(),
        "New space".into(),
        RowAction::Send(SendTarget::NewSpace),
    );
    new_space.glyph = "＋".into();
    new_space.detail = format!("named {space_name}");
    if plan::alone_in_space(snapshot, source) {
        new_space = new_space.disabled("already alone in this space");
        new_space.glyph = "＋".into();
    }
    if matches(filter, &format!("new space {space_name}")) {
        new_items.push(new_space);
    }
    push_section(&mut rows, Row::section("new", "new", ""), new_items);

    // This space's tabs; the current one is dimmed and marked "here".
    let mut tabs = Vec::new();
    for tab in snapshot.tabs_in(&source.workspace_id) {
        let label = names::tab_label(tab);
        if !matches(filter, &label) {
            continue;
        }
        let mut row = Row::item(
            format!("tab:{}", tab.tab_id),
            label,
            RowAction::Send(SendTarget::Tab(tab.tab_id.clone())),
        );
        if tab.tab_id == source.tab_id {
            row = row.disabled("");
            row.hint = "here".into();
        } else {
            row.detail = pane_count(tab.pane_count);
        }
        tabs.push(row);
    }
    let this_space = snapshot
        .workspace(&source.workspace_id)
        .map(|s| names::space_label(snapshot, s))
        .unwrap_or_default();
    push_section(
        &mut rows,
        Row::section("this", "this space", &format!(" · {this_space}")),
        tabs,
    );

    // Other spaces; → or ⏎ opens one.
    let mut spaces = Vec::new();
    for space in &snapshot.workspaces {
        if space.workspace_id == source.workspace_id {
            continue;
        }
        let label = names::space_label(snapshot, space);
        let tab_words: Vec<String> = snapshot
            .tabs_in(&space.workspace_id)
            .iter()
            .map(|t| names::tab_label(t))
            .collect();
        if !matches(filter, &format!("{label} {}", tab_words.join(" "))) {
            continue;
        }
        let mut row = Row::item(
            format!("space:{}", space.workspace_id),
            label,
            RowAction::OpenSpace(space.workspace_id.clone()),
        );
        row.hint = "→".into();
        row.selected_hint = "→".into();
        if let Some(warning) = plan::closes_space_warning(snapshot, source, &space.workspace_id) {
            row.detail = warning;
        }
        spaces.push(row);
    }
    push_section(&mut rows, Row::section("other", "other spaces", ""), spaces);
    if rows.is_empty() {
        rows.push(Row::message("Nothing matches"));
    }
    rows
}

/// Rows of the Fetch window: every pane in every space, grouped space →
/// tab → pane (design: "Fetch", edge cases 2.1, 2.11, 2.13, 2.14, 6.16).
pub fn fetch_rows(
    snapshot: &Snapshot,
    you: &PaneInfo,
    commands: &HashMap<String, Command>,
    filter: &str,
) -> Vec<Row> {
    if !plan::anything_to_fetch(snapshot, you) {
        return vec![Row::message(plan::NOTHING_TO_FETCH)];
    }
    let mut rows = Vec::new();
    for space in &snapshot.workspaces {
        let space_label = names::space_label(snapshot, space);
        let mut items = Vec::new();
        for tab in snapshot.tabs_in(&space.workspace_id) {
            let tab_label = names::tab_label(tab);
            let here = tab.tab_id == you.tab_id;
            let mut pane_rows = Vec::new();
            for pane in snapshot.panes_in_tab(&tab.tab_id) {
                let words = names::pane_words(pane, commands.get(&pane.terminal_id));
                let haystack = format!(
                    "{} {} {} {tab_label} {space_label}",
                    words.who, words.title, words.folder
                );
                if !matches(filter, &haystack) {
                    continue;
                }
                let mut row = Row::item(
                    format!("pane:{}", pane.terminal_id),
                    words.who,
                    RowAction::FetchPane(pane.pane_id.clone()),
                );
                row.indent = 4;
                row.glyph = words.glyph.to_string();
                row.status = pane.agent_status.clone();
                row.title = words.title;
                row.detail = words.folder;
                if here {
                    row.style = RowStyle::Dim;
                    row.selectable = false;
                    row.selected_hint.clear();
                    if pane.pane_id == you.pane_id {
                        row.detail = "you".into();
                    }
                }
                pane_rows.push(row);
            }
            let tab_matches = matches(filter, &format!("{tab_label} {space_label}"));
            if pane_rows.is_empty() && !tab_matches {
                continue;
            }
            let mut tab_row = Row::item(
                format!("tab:{}", tab.tab_id),
                tab_label,
                RowAction::FetchTab(tab.tab_id.clone()),
            );
            tab_row.indent = 2;
            tab_row.selected_hint = "⏎ whole tab".into();
            if here {
                tab_row.style = RowStyle::Dim;
                tab_row.selectable = false;
                tab_row.hint = "here".into();
                tab_row.selected_hint.clear();
            }
            items.push(tab_row);
            items.extend(pane_rows);
        }
        if !items.is_empty() {
            rows.push(Row::section(
                &format!("space:{}", space.workspace_id),
                &space_label,
                "",
            ));
            rows.extend(items);
        }
    }
    if rows.is_empty() {
        rows.push(Row::message("Nothing matches"));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api;
    use crate::sim::{Sim, SAMPLE_FIXTURE};

    fn setup() -> (Sim, Snapshot) {
        let sim = Sim::from_json(SAMPLE_FIXTURE).unwrap();
        let snapshot = api::snapshot(&sim).unwrap();
        (sim, snapshot)
    }

    fn names_of(rows: &[Row]) -> Vec<String> {
        rows.iter().map(|r| r.name.clone()).collect()
    }

    #[test]
    fn send_top_level_matches_the_design() {
        let (sim, snap) = setup();
        let source = snap.pane(&sim.pane_id("portfolio")).unwrap();
        let rows = send_rows(&snap, source, &HashMap::new(), "", None);
        assert_eq!(
            names_of(&rows),
            [
                "new",
                "New tab here",
                "New space",
                "this space",
                "Landing_Page_Copy",
                "API refactor",
                "Load tests",
                "other spaces",
                "Payments_Service_Rewrite"
            ]
        );
        assert_eq!(rows[1].detail, "named claude, next to this tab");
        assert_eq!(rows[2].detail, "named acme-web");
        assert_eq!(rows[3].note, " · Studio");
        // Edge case 1.8: the current tab is dimmed, marked here, unselectable.
        assert_eq!(
            (rows[4].style, rows[4].hint.as_str(), rows[4].selectable),
            (RowStyle::Dim, "here", false)
        );
        assert_eq!(rows[5].detail, "1 pane");
        assert_eq!(rows[8].hint, "→");
    }

    /// Edge case 1.9: a pane alone in its tab cannot use "New tab here".
    #[test]
    fn edge_1_9_new_tab_here_disabled_when_alone() {
        let (sim, snap) = setup();
        let source = snap.pane(&sim.pane_id("zsh")).unwrap();
        let rows = send_rows(&snap, source, &HashMap::new(), "", None);
        let row = rows.iter().find(|r| r.key == "new-tab").unwrap();
        assert!(!row.selectable);
        assert_eq!(row.detail, "already alone in this tab");
    }

    /// Drilling into another space shows its tabs and a new-tab row.
    #[test]
    fn send_drilled_into_other_space() {
        let (sim, snap) = setup();
        let source = snap.pane(&sim.pane_id("portfolio")).unwrap();
        let rows = send_rows(&snap, source, &HashMap::new(), "", Some("w2"));
        assert_eq!(
            names_of(&rows),
            [
                "Payments_Service_Rewrite",
                "new",
                "New tab in Payments_Service_Rewrite",
                "tabs",
                "deploying"
            ]
        );
        assert_eq!(rows[0].hint, "← back");
        assert_eq!(rows[2].detail, "named claude");
    }

    /// Edge case 1.7: rows into another space warn when the pane's own
    /// space would close.
    #[test]
    fn edge_1_7_rows_warn_about_closing_space() {
        let (sim, snap) = setup();
        let source = snap.pane(&sim.pane_id("incident")).unwrap();
        let rows = send_rows(&snap, source, &HashMap::new(), "", None);
        let career = rows.iter().find(|r| r.name == "Studio").unwrap();
        assert_eq!(career.detail, "closes Payments_Service_Rewrite");
        let drilled = send_rows(&snap, source, &HashMap::new(), "", Some("w1"));
        assert!(drilled[2]
            .detail
            .ends_with("closes Payments_Service_Rewrite"));
    }

    /// Edge case 6.16: with no other space, the section is hidden.
    #[test]
    fn edge_6_16_send_hides_other_spaces_when_none() {
        let sim = Sim::from_json(
            r#"{"spaces":[{"label":"s","tabs":[{"layout":{"split":"right",
            "first":{"pane":{"name":"a","active":true}},"second":{"pane":{"name":"b"}}}}]}]}"#,
        )
        .unwrap();
        let snap = api::snapshot(&sim).unwrap();
        let rows = send_rows(
            &snap,
            snap.pane(&sim.pane_id("a")).unwrap(),
            &HashMap::new(),
            "",
            None,
        );
        assert!(!rows.iter().any(|r| r.name == "other spaces"));
        assert!(rows
            .iter()
            .any(|r| r.name == "New tab here" && r.selectable));
    }

    /// Edge case 2.11 for Send: typing narrows every list at once, and a
    /// tab name in another space surfaces that space.
    #[test]
    fn send_filter_narrows_everything() {
        let (sim, snap) = setup();
        let source = snap.pane(&sim.pane_id("portfolio")).unwrap();
        let rows = send_rows(&snap, source, &HashMap::new(), "load", None);
        assert_eq!(names_of(&rows), ["this space", "Load tests"]);
        let rows = send_rows(&snap, source, &HashMap::new(), "deploy", None);
        assert_eq!(
            names_of(&rows),
            ["other spaces", "Payments_Service_Rewrite"]
        );
        let rows = send_rows(&snap, source, &HashMap::new(), "zzz", None);
        assert_eq!(names_of(&rows), ["Nothing matches"]);
    }

    #[test]
    fn fetch_groups_space_tab_pane_by_name() {
        let (sim, snap) = setup();
        let you = snap.pane(&sim.pane_id("portfolio")).unwrap();
        let mut commands = HashMap::new();
        commands.insert(
            sim.terminal("scraper"),
            Command {
                display: "python3 scrape_docs.py".into(),
                program: "python3".into(),
            },
        );
        let rows = fetch_rows(&snap, you, &commands, "");
        assert_eq!(
            names_of(&rows),
            [
                "Studio",
                "Landing_Page_Copy",
                "claude",
                "claude",
                "API refactor",
                "shell",
                "Load tests",
                "python3 scrape_docs.py",
                "Payments_Service_Rewrite",
                "deploying",
                "claude"
            ]
        );
        // Edge case 2.1: this tab and its panes are dimmed and unselectable.
        assert!(!rows[1].selectable && rows[1].hint == "here");
        assert!(!rows[2].selectable && !rows[3].selectable);
        assert_eq!(rows[3].detail, "you");
        assert_eq!(rows[2].title, "headline rewrite");
        // Tab rows fetch the whole tab.
        assert_eq!(rows[4].selected_hint, "⏎ whole tab");
        assert_eq!(rows[5].detail, "acme-web");
        // Edge case 2.13: raw ids never appear.
        let text = format!("{rows:?}");
        assert!(!text.contains("name: \"w1:p"), "{text}");
    }

    /// Edge case 2.11: every word must match a name, agent, command, title
    /// or folder; space, tab and pane rows narrow together.
    #[test]
    fn edge_2_11_fetch_filter_words() {
        let (sim, snap) = setup();
        let you = snap.pane(&sim.pane_id("portfolio")).unwrap();
        let rows = fetch_rows(&snap, you, &HashMap::new(), "rollback claude");
        assert_eq!(
            names_of(&rows),
            ["Payments_Service_Rewrite", "deploying", "claude"]
        );
        let rows = fetch_rows(&snap, you, &HashMap::new(), "LOAD");
        assert_eq!(names_of(&rows), ["Studio", "Load tests", "shell"]);
        let rows = fetch_rows(&snap, you, &HashMap::new(), "w1:p");
        assert_eq!(names_of(&rows), ["Nothing matches"]);
    }

    /// Edge case 6.16: nothing to fetch.
    #[test]
    fn edge_6_16_fetch_nothing_to_fetch() {
        let sim = Sim::from_json(
            r#"{"spaces":[{"label":"s","tabs":[{"layout":{"pane":{"name":"a","active":true}}}]}]}"#,
        )
        .unwrap();
        let snap = api::snapshot(&sim).unwrap();
        let rows = fetch_rows(
            &snap,
            snap.pane(&sim.pane_id("a")).unwrap(),
            &HashMap::new(),
            "",
        );
        assert_eq!(names_of(&rows), [plan::NOTHING_TO_FETCH]);
    }
}
