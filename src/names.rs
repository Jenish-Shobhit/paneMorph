//! How paneMorph names panes, tabs and spaces.

use std::collections::HashSet;

use crate::model::{basename, Command, PaneInfo, Snapshot, TabInfo, WorkspaceInfo};

/// Longest tab or space name paneMorph creates (edge case 3.5).
pub const MAX_NAME: usize = 32;

/// Clean a name: drop control characters, collapse whitespace, cap at 32
/// characters with "…" (edge case 3.5).
pub fn sanitize(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate(&collapsed, MAX_NAME)
}

/// Cut to `max` characters, ending in "…" when cut.
pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out: String = text.chars().take(max - 1).collect();
    out = out.trim_end().to_string();
    out.push('…');
    out
}

fn non_empty(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// The agent name: `display_agent`, else `agent`.
pub fn agent(pane: &PaneInfo) -> Option<&str> {
    non_empty(&pane.display_agent).or_else(|| non_empty(&pane.agent))
}

/// The pane title: `label`, else metadata `title`, else the stripped
/// terminal title (edge case 2.13).
pub fn title(pane: &PaneInfo) -> Option<&str> {
    non_empty(&pane.label)
        .or_else(|| non_empty(&pane.title))
        .or_else(|| non_empty(&pane.terminal_title_stripped))
}

/// Basename of `foreground_cwd`, else `cwd`.
pub fn folder(pane: &PaneInfo) -> Option<String> {
    non_empty(&pane.foreground_cwd)
        .or_else(|| non_empty(&pane.cwd))
        .map(basename)
        .filter(|s| !s.is_empty())
}

/// The words a row shows for a pane (edge case 2.13). Never a raw id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneWords {
    pub glyph: &'static str,
    /// Agent, else running command, else "shell".
    pub who: String,
    pub title: String,
    pub folder: String,
}

pub fn pane_words(pane: &PaneInfo, command: Option<&Command>) -> PaneWords {
    let who = agent(pane)
        .map(str::to_string)
        .or_else(|| command.map(|c| c.display.clone()))
        .unwrap_or_else(|| "shell".to_string());
    let mut title = title(pane).unwrap_or_default().to_string();
    // A title that only repeats the agent or command adds nothing.
    if title.eq_ignore_ascii_case(&who)
        || command.is_some_and(|c| title.eq_ignore_ascii_case(&c.program))
    {
        title.clear();
    }
    PaneWords {
        glyph: status_glyph(&pane.agent_status),
        who,
        title,
        folder: folder(pane).unwrap_or_default(),
    }
}

/// herdr's status dots: working, blocked, done, idle.
pub fn status_glyph(status: &str) -> &'static str {
    match status {
        "working" | "blocked" | "done" => "●",
        _ => "○",
    }
}

/// Name for a new tab made from `pane` (edge case 3.1): agent, else the
/// running program, else the folder.
pub fn tab_base_name(pane: &PaneInfo, command: Option<&Command>) -> String {
    // An idle shell is not a running command, so its folder names the tab.
    let raw = agent(pane)
        .map(str::to_string)
        .or_else(|| command.filter(|c| !c.is_shell).map(|c| c.program.clone()))
        .or_else(|| folder(pane))
        .unwrap_or_else(|| "pane".to_string());
    sanitize(&raw)
}

/// Name for a new space made from `pane` (edge case 3.3): its folder.
pub fn space_base_name(pane: &PaneInfo, command: Option<&Command>) -> String {
    folder(pane)
        .map(|f| sanitize(&f))
        .unwrap_or_else(|| tab_base_name(pane, command))
}

/// First free name among `taken`: `base`, `base 2`, `base 3`… (3.2, 3.4).
pub fn unique_name(base: &str, taken: &HashSet<String>) -> String {
    if !taken.contains(base) {
        return base.to_string();
    }
    for n in 2.. {
        let suffix = format!(" {n}");
        let room = MAX_NAME.saturating_sub(suffix.chars().count());
        let stem = truncate(base, room);
        let candidate = format!("{stem}{suffix}");
        if !taken.contains(&candidate) {
            return candidate;
        }
    }
    unreachable!("an unbounded range always finds a free name")
}

/// Tab name that is free inside `workspace_id` (edge case 3.2).
pub fn unique_tab_name(snapshot: &Snapshot, workspace_id: &str, base: &str) -> String {
    let taken = snapshot
        .tabs_in(workspace_id)
        .iter()
        .map(|t| t.label.clone())
        .collect();
    unique_name(base, &taken)
}

/// Space name that is free in the sidebar (edge case 3.4).
pub fn unique_space_name(snapshot: &Snapshot, base: &str) -> String {
    let taken = snapshot
        .workspaces
        .iter()
        .map(|w| w.label.clone())
        .collect();
    unique_name(base, &taken)
}

/// A tab's label as herdr's tab bar shows it (edge case 2.14).
pub fn tab_label(tab: &TabInfo) -> String {
    if tab.label.trim().is_empty() {
        tab.number.to_string()
    } else {
        tab.label.clone()
    }
}

/// Whether a tab has a custom name. herdr labels untitled tabs with their
/// 1-based position, so a label equal to the position means "untitled".
pub fn tab_custom_label(snapshot: &Snapshot, tab: &TabInfo) -> Option<String> {
    let position = snapshot.tab_index(&tab.tab_id)? + 1;
    (tab.label != position.to_string()).then(|| tab.label.clone())
}

/// A space's label, plus its sidebar number when another space shares the
/// label (edge case 2.14).
pub fn space_label(snapshot: &Snapshot, space: &WorkspaceInfo) -> String {
    let shared = snapshot
        .workspaces
        .iter()
        .filter(|w| w.label == space.label)
        .count()
        > 1;
    let label = if space.label.trim().is_empty() {
        format!("space {}", space.number)
    } else {
        space.label.clone()
    };
    if shared {
        format!("{label} ({})", space.number)
    } else {
        label
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane() -> PaneInfo {
        PaneInfo {
            pane_id: "w4:pM".into(),
            workspace_id: "w4".into(),
            tab_id: "w4:t8".into(),
            cwd: Some("/home/dev/code/acme-web".into()),
            ..Default::default()
        }
    }

    /// Edge case 2.13: an untitled pane without an agent shows "shell" and
    /// its folder, never its raw id.
    #[test]
    fn edge_2_13_untitled_pane_shows_shell_and_folder_not_id() {
        let words = pane_words(&pane(), None);
        assert_eq!(words.who, "shell");
        assert_eq!(words.folder, "acme-web");
        assert!(!format!("{words:?}").contains("w4:pM"));
    }

    /// Edge case 2.13: a running command replaces "shell".
    #[test]
    fn edge_2_13_command_names_a_pane_without_agent() {
        let command = Command {
            display: "python3 scrape_docs.py".into(),
            program: "python3".into(),
            is_shell: false,
        };
        assert_eq!(
            pane_words(&pane(), Some(&command)).who,
            "python3 scrape_docs.py"
        );
    }

    /// Edge case 2.13: agent first (display_agent over agent), then title
    /// from label, metadata title, terminal title; folder prefers foreground_cwd.
    #[test]
    fn edge_2_13_field_precedence() {
        let mut p = pane();
        p.agent = Some("claude".into());
        p.display_agent = Some("Claude".into());
        p.terminal_title_stripped = Some("term".into());
        p.title = Some("meta".into());
        p.foreground_cwd = Some("/x/portfolio".into());
        let words = pane_words(&p, None);
        assert_eq!(
            (
                words.who.as_str(),
                words.title.as_str(),
                words.folder.as_str()
            ),
            ("Claude", "meta", "portfolio")
        );
        p.label = Some("headline rewrite".into());
        assert_eq!(pane_words(&p, None).title, "headline rewrite");
    }

    /// Edge case 3.1: agent, else program, else folder.
    #[test]
    fn edge_3_1_new_tab_name_rule() {
        let mut p = pane();
        assert_eq!(tab_base_name(&p, None), "acme-web");
        let command = Command {
            display: "python3 x.py".into(),
            program: "python3".into(),
            is_shell: false,
        };
        assert_eq!(tab_base_name(&p, Some(&command)), "python3");
        p.agent = Some("claude".into());
        assert_eq!(tab_base_name(&p, Some(&command)), "claude");
    }

    /// Edge cases 3.2 and 3.4: " 2", " 3" suffixes take the first free name.
    #[test]
    fn edge_3_2_and_3_4_numeric_suffixes() {
        let taken: HashSet<String> = ["claude", "claude 2"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(unique_name("claude", &taken), "claude 3");
        assert_eq!(unique_name("zsh", &taken), "zsh");
    }

    /// Edge case 3.3: a new space is named after the folder.
    #[test]
    fn edge_3_3_space_named_after_folder() {
        let mut p = pane();
        p.agent = Some("claude".into());
        assert_eq!(space_base_name(&p, None), "acme-web");
    }

    /// Edge case 3.5: control characters removed, whitespace collapsed,
    /// long names cut to 32 characters with "…".
    #[test]
    fn edge_3_5_sanitize() {
        assert_eq!(sanitize("  a\tb\x07\n  c  "), "a b c");
        let long = "x".repeat(40);
        let cut = sanitize(&long);
        assert_eq!(cut.chars().count(), 32);
        assert!(cut.ends_with('…'));
        let taken: HashSet<String> = [cut.clone()].into_iter().collect();
        assert!(unique_name(&cut, &taken).chars().count() <= 32);
    }

    /// Edge case 2.14: duplicate space labels gain the sidebar number.
    #[test]
    fn edge_2_14_duplicate_space_labels_get_numbers() {
        let snapshot = Snapshot {
            workspaces: vec![
                WorkspaceInfo {
                    workspace_id: "w1".into(),
                    number: 1,
                    label: "api".into(),
                    ..Default::default()
                },
                WorkspaceInfo {
                    workspace_id: "w2".into(),
                    number: 2,
                    label: "api".into(),
                    ..Default::default()
                },
                WorkspaceInfo {
                    workspace_id: "w3".into(),
                    number: 3,
                    label: "web".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert_eq!(space_label(&snapshot, &snapshot.workspaces[1]), "api (2)");
        assert_eq!(space_label(&snapshot, &snapshot.workspaces[2]), "web");
    }

    /// Edge case 3.12: untitled tabs are recognised by their position label.
    #[test]
    fn edge_3_12_untitled_tab_detection_uses_position() {
        let snapshot = Snapshot {
            tabs: vec![
                TabInfo {
                    tab_id: "w1:t8".into(),
                    workspace_id: "w1".into(),
                    number: 8,
                    label: "1".into(),
                    ..Default::default()
                },
                TabInfo {
                    tab_id: "w1:t12".into(),
                    workspace_id: "w1".into(),
                    number: 12,
                    label: "docs".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert_eq!(tab_custom_label(&snapshot, &snapshot.tabs[0]), None);
        assert_eq!(
            tab_custom_label(&snapshot, &snapshot.tabs[1]),
            Some("docs".into())
        );
    }
}
