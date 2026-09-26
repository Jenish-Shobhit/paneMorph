//! Typed views of herdr's socket API (protocol 22, herdr 0.9.0).
//! Fields paneMorph does not need are ignored; missing optional fields
//! default, so newer herdr versions keep parsing.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SplitDir {
    #[default]
    Right,
    Down,
}

impl SplitDir {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Down => "down",
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            Self::Right => Self::Down,
            Self::Down => Self::Right,
        }
    }

    /// The word shown in the windows: "right" or "below".
    pub fn word(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Down => "below",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub protocol: u32,
    #[serde(default)]
    pub focused_workspace_id: Option<String>,
    #[serde(default)]
    pub focused_tab_id: Option<String>,
    #[serde(default)]
    pub focused_pane_id: Option<String>,
    #[serde(default)]
    pub workspaces: Vec<WorkspaceInfo>,
    #[serde(default)]
    pub tabs: Vec<TabInfo>,
    #[serde(default)]
    pub panes: Vec<PaneInfo>,
    #[serde(default)]
    pub layouts: Vec<LayoutSnapshot>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub workspace_id: String,
    #[serde(default)]
    pub number: u32,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub pane_count: usize,
    #[serde(default)]
    pub tab_count: usize,
    #[serde(default)]
    pub active_tab_id: String,
    #[serde(default)]
    pub agent_status: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TabInfo {
    pub tab_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub number: u32,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub pane_count: usize,
    #[serde(default)]
    pub agent_status: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PaneInfo {
    pub pane_id: String,
    #[serde(default)]
    pub terminal_id: String,
    pub workspace_id: String,
    pub tab_id: String,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub agent_status: String,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub display_agent: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub terminal_title_stripped: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub foreground_cwd: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LayoutPane {
    pub pane_id: String,
    #[serde(default)]
    pub focused: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LayoutSplit {
    #[serde(default)]
    pub direction: SplitDir,
    #[serde(default)]
    pub ratio: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LayoutSnapshot {
    pub workspace_id: String,
    pub tab_id: String,
    #[serde(default)]
    pub zoomed: bool,
    #[serde(default)]
    pub focused_pane_id: String,
    #[serde(default)]
    pub panes: Vec<LayoutPane>,
    #[serde(default)]
    pub splits: Vec<LayoutSplit>,
}

/// `layout.export` tree node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum LayoutNode {
    Pane {
        #[serde(default)]
        pane_id: Option<String>,
    },
    Split {
        direction: SplitDir,
        ratio: f64,
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutDescription {
    pub workspace_id: String,
    pub tab_id: String,
    #[serde(default)]
    pub zoomed: bool,
    #[serde(default)]
    pub focused_pane_id: String,
    pub root: LayoutNode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoveResult {
    pub changed: bool,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub previous_pane_id: String,
    #[serde(default)]
    pub previous_workspace_id: String,
    #[serde(default)]
    pub previous_tab_id: String,
    pub pane: PaneInfo,
    #[serde(default)]
    pub created_workspace: Option<WorkspaceInfo>,
    #[serde(default)]
    pub created_tab: Option<TabInfo>,
    #[serde(default)]
    pub closed_workspace_id: Option<String>,
    #[serde(default)]
    pub closed_tab_id: Option<String>,
    #[serde(default)]
    pub focused_pane_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProcessEntry {
    #[serde(default)]
    pub pid: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub argv: Option<Vec<String>>,
    #[serde(default)]
    pub argv0: Option<String>,
    #[serde(default)]
    pub cmdline: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProcessInfo {
    #[serde(default)]
    pub pane_id: String,
    #[serde(default)]
    pub shell_pid: Option<u32>,
    #[serde(default)]
    pub foreground_process_group_id: Option<u32>,
    #[serde(default)]
    pub foreground_processes: Vec<ProcessEntry>,
}

/// What a pane is running, as paneMorph names it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Command {
    /// The full command line, e.g. `python3 scrape_docs.py`.
    pub display: String,
    /// The program name, e.g. `python3`.
    pub program: String,
    /// The pane's own shell sits at its prompt: nothing is running.
    pub is_shell: bool,
}

/// Interactive shells: a pane showing one of these is idle at a prompt.
pub const SHELLS: &[&str] = &[
    "zsh", "bash", "fish", "sh", "dash", "ksh", "tcsh", "csh", "nu", "elvish", "xonsh", "pwsh",
];

impl ProcessInfo {
    /// The foreground command: the process-group leader when known.
    pub fn command(&self) -> Option<Command> {
        let leader = self
            .foreground_process_group_id
            .and_then(|pgid| self.foreground_processes.iter().find(|p| p.pid == pgid))
            .or_else(|| self.foreground_processes.first())?;
        let argv = leader.argv.clone().unwrap_or_default();
        let program_path = argv
            .first()
            .cloned()
            .or_else(|| leader.argv0.clone())
            .unwrap_or_else(|| leader.name.clone());
        let program = basename(program_path.trim_start_matches('-'));
        let display = match (&leader.cmdline, argv.is_empty()) {
            (Some(line), _) if !line.trim().is_empty() => {
                let mut words = line.split_whitespace();
                let first = words
                    .next()
                    .map(|word| basename(word.trim_start_matches('-')))
                    .unwrap_or_default();
                std::iter::once(first)
                    .chain(words.map(str::to_string))
                    .collect::<Vec<_>>()
                    .join(" ")
            }
            (_, false) => std::iter::once(basename(argv[0].trim_start_matches('-')))
                .chain(argv[1..].iter().cloned())
                .collect::<Vec<_>>()
                .join(" "),
            _ => program.clone(),
        };
        if program.is_empty() {
            return None;
        }
        let is_shell = Some(leader.pid) == self.shell_pid || SHELLS.contains(&program.as_str());
        Some(Command {
            display,
            program,
            is_shell,
        })
    }
}

pub fn basename(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    trimmed
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(trimmed)
        .to_string()
}

impl Snapshot {
    pub fn pane(&self, pane_id: &str) -> Option<&PaneInfo> {
        self.panes.iter().find(|p| p.pane_id == pane_id)
    }

    pub fn pane_by_terminal(&self, terminal_id: &str) -> Option<&PaneInfo> {
        self.panes.iter().find(|p| p.terminal_id == terminal_id)
    }

    pub fn tab(&self, tab_id: &str) -> Option<&TabInfo> {
        self.tabs.iter().find(|t| t.tab_id == tab_id)
    }

    pub fn workspace(&self, workspace_id: &str) -> Option<&WorkspaceInfo> {
        self.workspaces
            .iter()
            .find(|w| w.workspace_id == workspace_id)
    }

    pub fn layout(&self, tab_id: &str) -> Option<&LayoutSnapshot> {
        self.layouts.iter().find(|l| l.tab_id == tab_id)
    }

    /// Tabs of one space in tab-bar order (edge case 4.3).
    pub fn tabs_in(&self, workspace_id: &str) -> Vec<&TabInfo> {
        self.tabs
            .iter()
            .filter(|t| t.workspace_id == workspace_id)
            .collect()
    }

    /// Position of a tab in its space's tab bar.
    pub fn tab_index(&self, tab_id: &str) -> Option<usize> {
        let tab = self.tab(tab_id)?;
        self.tabs_in(&tab.workspace_id)
            .iter()
            .position(|t| t.tab_id == tab_id)
    }

    pub fn workspace_index(&self, workspace_id: &str) -> Option<usize> {
        self.workspaces
            .iter()
            .position(|w| w.workspace_id == workspace_id)
    }

    /// Panes of a tab in layout (reading) order.
    pub fn panes_in_tab(&self, tab_id: &str) -> Vec<&PaneInfo> {
        match self.layout(tab_id) {
            Some(layout) if !layout.panes.is_empty() => layout
                .panes
                .iter()
                .filter_map(|lp| self.pane(&lp.pane_id))
                .collect(),
            _ => self.panes.iter().filter(|p| p.tab_id == tab_id).collect(),
        }
    }

    pub fn pane_count_in_tab(&self, tab_id: &str) -> usize {
        self.panes.iter().filter(|p| p.tab_id == tab_id).count()
    }

    pub fn pane_count_in_space(&self, workspace_id: &str) -> usize {
        self.panes
            .iter()
            .filter(|p| p.workspace_id == workspace_id)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_herdr_snapshot_and_ignores_unknown_fields() {
        let raw = r#"{"version":"0.9.0","protocol":22,"focused_pane_id":"w1:p1",
            "workspaces":[{"workspace_id":"w1","number":1,"label":"Studio","focused":true,
              "pane_count":1,"tab_count":1,"active_tab_id":"w1:t1","agent_status":"idle","extra":1}],
            "tabs":[{"tab_id":"w1:t1","workspace_id":"w1","number":8,"label":"1","focused":true,
              "pane_count":1,"agent_status":"idle"}],
            "panes":[{"pane_id":"w1:p1","terminal_id":"t-1","workspace_id":"w1","tab_id":"w1:t1",
              "focused":true,"agent_status":"idle","revision":3,"tokens":{}}],
            "layouts":[{"workspace_id":"w1","tab_id":"w1:t1","zoomed":false,"focused_pane_id":"w1:p1",
              "area":{"x":0,"y":0,"width":10,"height":10},
              "panes":[{"pane_id":"w1:p1","focused":true,"rect":{"x":0,"y":0,"width":10,"height":10}}],
              "splits":[]}],"agents":[]}"#;
        let snapshot: Snapshot = serde_json::from_str(raw).unwrap();
        assert_eq!(snapshot.tabs[0].number, 8);
        assert_eq!(snapshot.tab_index("w1:t1"), Some(0));
        assert_eq!(snapshot.panes_in_tab("w1:t1").len(), 1);
    }

    #[test]
    fn layout_export_tree_parses() {
        let raw = r#"{"workspace_id":"w1","tab_id":"w1:t2","zoomed":false,"focused_pane_id":"w1:p3",
          "root":{"type":"split","direction":"down","ratio":0.6,
            "first":{"type":"pane","pane_id":"w1:p2","cwd":"/tmp"},
            "second":{"type":"pane","pane_id":"w1:p3"}}}"#;
        let layout: LayoutDescription = serde_json::from_str(raw).unwrap();
        assert!(matches!(
            layout.root,
            LayoutNode::Split {
                direction: SplitDir::Down,
                ..
            }
        ));
    }

    #[test]
    fn process_command_prefers_group_leader_and_basenames() {
        let info = ProcessInfo {
            foreground_process_group_id: Some(20),
            foreground_processes: vec![
                ProcessEntry {
                    pid: 21,
                    name: "cat".into(),
                    ..Default::default()
                },
                ProcessEntry {
                    pid: 20,
                    name: "python3".into(),
                    argv: Some(vec!["/usr/bin/python3".into(), "scrape_docs.py".into()]),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let command = info.command().unwrap();
        assert_eq!(command.program, "python3");
        assert_eq!(command.display, "python3 scrape_docs.py");
    }

    #[test]
    fn login_shell_dash_is_stripped() {
        let info = ProcessInfo {
            foreground_processes: vec![ProcessEntry {
                pid: 1,
                name: "zsh".into(),
                argv: Some(vec!["-zsh".into()]),
                ..Default::default()
            }],
            ..Default::default()
        };
        let command = info.command().unwrap();
        assert_eq!(command.program, "zsh");
        assert!(command.is_shell);
    }
}
