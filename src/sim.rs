//! An in-memory herdr that answers the socket methods paneMorph uses.
//!
//! It mirrors herdr 0.9.0's documented and source-verified behaviour:
//! `pane.move` refuses zoomed and same-tab moves, removes emptied tabs,
//! removes a space emptied by a cross-space move, gives a moved pane a new
//! public id in another space (keeping the old id as an alias), appends new
//! tabs and spaces at the end, and inserts the moved pane as the second
//! child of a new split. Unit tests and `panemorph preview` run on it.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Mutex;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::{Herdr, HerdrError};
use crate::model::{basename, SplitDir};
use crate::topology::Tree;

// ---------------------------------------------------------------------------
// Fixture format (also accepted by `panemorph preview --fixture`).

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Fixture {
    #[serde(default)]
    pub version: Option<String>,
    pub spaces: Vec<FxSpace>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FxSpace {
    pub label: String,
    pub tabs: Vec<FxTab>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FxTab {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub zoomed: bool,
    pub layout: FxNode,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum FxNode {
    Pane {
        pane: Box<FxPane>,
    },
    Split {
        split: SplitDir,
        #[serde(default = "half")]
        ratio: f64,
        first: Box<FxNode>,
        second: Box<FxNode>,
    },
}

fn half() -> f64 {
    0.5
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct FxPane {
    /// Test handle; defaults to `p<n>`.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub display_agent: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub terminal_title: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub foreground_cwd: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub command: Option<Vec<String>>,
    /// The focused pane of its tab.
    #[serde(default)]
    pub focused: bool,
    /// The session's focused pane (its space and tab become active).
    #[serde(default)]
    pub active: bool,
}

// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Fault {
    /// The n-th `pane.move` (0-based, counted over the sim's life) fails.
    FailMove(usize),
    /// The n-th `pane.move` happens but its reply is lost.
    LoseMoveReply(usize),
    /// Every `tab.move` fails.
    FailTabMove,
}

#[derive(Debug, Clone)]
struct Term {
    handle: String,
    pane: FxPane,
}

#[derive(Debug, Clone)]
struct Tab {
    number: usize,
    custom: Option<String>,
    root: Tree,
    focused: String,
    zoomed: bool,
}

#[derive(Debug, Clone)]
struct Space {
    id: String,
    label: String,
    tabs: Vec<Tab>,
    active_tab: usize,
    next_pane: usize,
    next_tab: usize,
    numbers: HashMap<String, usize>,
}

#[derive(Debug, Default)]
struct State {
    version: String,
    spaces: Vec<Space>,
    active: Option<usize>,
    terms: BTreeMap<String, Term>,
    handles: HashMap<String, String>,
    aliases: HashMap<String, String>,
    next_space: usize,
    next_term: usize,
    moves: usize,
    faults: Vec<Fault>,
    calls: Vec<(String, Value)>,
    toast: (bool, String),
    popup_busy: bool,
}

pub struct Sim {
    state: Mutex<State>,
}

fn err(code: &str, message: impl Into<String>) -> HerdrError {
    HerdrError::api(code, message)
}

fn tree_insert(tree: &Tree, target: &str, moved: &str, dir: SplitDir, ratio: f64) -> Tree {
    match tree {
        Tree::Leaf { id } if id == target => Tree::split(
            dir,
            ratio.clamp(0.1, 0.9),
            Tree::leaf(id),
            Tree::leaf(moved),
        ),
        Tree::Leaf { .. } => tree.clone(),
        Tree::Split {
            dir: d,
            ratio: r,
            first,
            second,
        } => Tree::split(
            *d,
            *r,
            tree_insert(first, target, moved, dir, ratio),
            tree_insert(second, target, moved, dir, ratio),
        ),
    }
}

fn tree_without(tree: &Tree, id: &str) -> Option<Tree> {
    let keep: HashSet<String> = tree
        .leaves()
        .into_iter()
        .filter(|l| *l != id)
        .map(str::to_string)
        .collect();
    tree.prune(&keep)
}

impl State {
    fn space_index(&self, id: &str) -> Option<usize> {
        self.spaces.iter().position(|s| s.id == id)
    }

    fn locate(&self, term: &str) -> Option<(usize, usize)> {
        self.spaces.iter().enumerate().find_map(|(si, s)| {
            s.tabs
                .iter()
                .position(|t| t.root.contains(term))
                .map(|ti| (si, ti))
        })
    }

    fn pane_public(&self, term: &str) -> Option<String> {
        let (si, _) = self.locate(term)?;
        let space = &self.spaces[si];
        Some(format!("{}:p{}", space.id, space.numbers.get(term)?))
    }

    fn tab_public(&self, si: usize, ti: usize) -> String {
        format!(
            "{}:t{}",
            self.spaces[si].id, self.spaces[si].tabs[ti].number
        )
    }

    fn resolve_pane(&self, pane_id: &str) -> Option<String> {
        if let Some((ws, number)) = pane_id.split_once(":p") {
            if let Some(space) = self.spaces.iter().find(|s| s.id == ws) {
                if let Some((term, _)) = space.numbers.iter().find(|(_, n)| n.to_string() == number)
                {
                    if self.locate(term).is_some() {
                        return Some(term.clone());
                    }
                }
            }
        }
        self.aliases
            .get(pane_id)
            .filter(|term| self.locate(term).is_some())
            .cloned()
    }

    fn resolve_tab(&self, tab_id: &str) -> Option<(usize, usize)> {
        let (ws, number) = tab_id.split_once(":t")?;
        let si = self.space_index(ws)?;
        let ti = self.spaces[si]
            .tabs
            .iter()
            .position(|t| t.number.to_string() == number)?;
        Some((si, ti))
    }

    fn register(&mut self, si: usize, term: &str) {
        let space = &mut self.spaces[si];
        if !space.numbers.contains_key(term) {
            space.numbers.insert(term.to_string(), space.next_pane);
            space.next_pane += 1;
        }
    }

    fn tab_label(&self, si: usize, ti: usize) -> String {
        self.spaces[si].tabs[ti]
            .custom
            .clone()
            .unwrap_or_else(|| (ti + 1).to_string())
    }

    fn pane_json(&self, term: &str) -> Value {
        let (si, ti) = self.locate(term).expect("pane_json on a live pane");
        let t = &self.terms[term];
        let tab = &self.spaces[si].tabs[ti];
        let focused =
            self.active == Some(si) && self.spaces[si].active_tab == ti && tab.focused == term;
        json!({
            "pane_id": self.pane_public(term),
            "terminal_id": term,
            "workspace_id": self.spaces[si].id,
            "tab_id": self.tab_public(si, ti),
            "focused": focused,
            "agent_status": t.pane.status.clone().unwrap_or_else(|| "idle".into()),
            "agent": t.pane.agent,
            "display_agent": t.pane.display_agent,
            "label": t.pane.label,
            "title": t.pane.title,
            "terminal_title_stripped": t.pane.terminal_title,
            "cwd": t.pane.cwd,
            "foreground_cwd": t.pane.foreground_cwd,
            "revision": 1,
        })
    }

    fn tab_json(&self, si: usize, ti: usize) -> Value {
        let tab = &self.spaces[si].tabs[ti];
        json!({
            "tab_id": self.tab_public(si, ti),
            "workspace_id": self.spaces[si].id,
            "number": tab.number,
            "label": self.tab_label(si, ti),
            "focused": self.active == Some(si) && self.spaces[si].active_tab == ti,
            "pane_count": tab.root.leaves().len(),
            "agent_status": "idle",
        })
    }

    fn space_json(&self, si: usize) -> Value {
        let space = &self.spaces[si];
        json!({
            "workspace_id": space.id,
            "number": si + 1,
            "label": space.label,
            "focused": self.active == Some(si),
            "pane_count": space.tabs.iter().map(|t| t.root.leaves().len()).sum::<usize>(),
            "tab_count": space.tabs.len(),
            "active_tab_id": self.tab_public(si, space.active_tab.min(space.tabs.len().saturating_sub(1))),
            "agent_status": "idle",
        })
    }

    fn layout_json(&self, si: usize, ti: usize) -> Value {
        let tab = &self.spaces[si].tabs[ti];
        let panes: Vec<Value> = tab
            .root
            .leaves()
            .into_iter()
            .map(|term| json!({"pane_id": self.pane_public(term), "focused": tab.focused == term}))
            .collect();
        let mut splits = Vec::new();
        fn walk(node: &Tree, out: &mut Vec<Value>) {
            if let Tree::Split {
                dir,
                ratio,
                first,
                second,
            } = node
            {
                out.push(
                    json!({"id": out.len().to_string(), "direction": dir.as_str(), "ratio": ratio}),
                );
                walk(first, out);
                walk(second, out);
            }
        }
        walk(&tab.root, &mut splits);
        json!({
            "workspace_id": self.spaces[si].id,
            "tab_id": self.tab_public(si, ti),
            "zoomed": tab.zoomed,
            "focused_pane_id": self.pane_public(&tab.focused),
            "panes": panes,
            "splits": splits,
        })
    }

    fn snapshot_json(&self) -> Value {
        let mut tabs = Vec::new();
        let mut panes = Vec::new();
        let mut layouts = Vec::new();
        for (si, space) in self.spaces.iter().enumerate() {
            for (ti, tab) in space.tabs.iter().enumerate() {
                tabs.push(self.tab_json(si, ti));
                layouts.push(self.layout_json(si, ti));
                for term in tab.root.leaves() {
                    panes.push(self.pane_json(term));
                }
            }
        }
        let (fw, ft, fp) = match self.active {
            Some(si) if !self.spaces[si].tabs.is_empty() => {
                let ti = self.spaces[si].active_tab;
                (
                    Some(self.spaces[si].id.clone()),
                    Some(self.tab_public(si, ti)),
                    self.pane_public(&self.spaces[si].tabs[ti].focused),
                )
            }
            _ => (None, None, None),
        };
        json!({"snapshot": {
            "version": self.version,
            "protocol": 22,
            "focused_workspace_id": fw,
            "focused_tab_id": ft,
            "focused_pane_id": fp,
            "workspaces": (0..self.spaces.len()).map(|si| self.space_json(si)).collect::<Vec<_>>(),
            "tabs": tabs,
            "panes": panes,
            "layouts": layouts,
            "agents": [],
        }})
    }

    fn export_tree(&self, node: &Tree) -> Value {
        match node {
            Tree::Leaf { id } => json!({"type": "pane", "pane_id": self.pane_public(id)}),
            Tree::Split {
                dir,
                ratio,
                first,
                second,
            } => json!({
                "type": "split", "direction": dir.as_str(), "ratio": ratio,
                "first": self.export_tree(first),
                "second": self.export_tree(second),
            }),
        }
    }

    fn adjust_after_tab_removal(space: &mut Space, removed: usize) {
        if space.tabs.is_empty() {
            space.active_tab = 0;
        } else if removed < space.active_tab {
            space.active_tab -= 1;
        } else if removed == space.active_tab {
            space.active_tab = removed.saturating_sub(1).min(space.tabs.len() - 1);
        }
    }

    fn unchanged_move(&self, term: &str, reason: &str) -> Value {
        let (si, ti) = self.locate(term).unwrap();
        json!({"move_result": {
            "changed": false, "reason": reason,
            "previous_pane_id": self.pane_public(term),
            "previous_workspace_id": self.spaces[si].id,
            "previous_tab_id": self.tab_public(si, ti),
            "pane": self.pane_json(term),
            "target_layout": self.layout_json(si, ti),
            "focused_pane_id": self.pane_public(&self.spaces[si].tabs[ti].focused),
        }})
    }

    fn pane_move(&mut self, params: &Value) -> Result<Value, HerdrError> {
        let pane_id = params["pane_id"].as_str().unwrap_or_default();
        let term = self
            .resolve_pane(pane_id)
            .ok_or_else(|| err("pane_not_found", "source pane not found"))?;
        let (ssi, sti) = self.locate(&term).unwrap();
        if self.spaces[ssi].tabs[sti].zoomed {
            return Ok(self.unchanged_move(&term, "zoomed_tab"));
        }
        let dest = &params["destination"];
        let focus = params["focus"].as_bool().unwrap_or(false);
        enum Resolved {
            Tab(String, String, SplitDir, f64),
            NewTab(String, Option<String>),
            NewSpace(Option<String>, Option<String>),
        }
        let resolved = match dest["type"].as_str() {
            Some("tab") => {
                let tab_id = dest["tab_id"].as_str().unwrap_or_default();
                let (tsi, tti) = self
                    .resolve_tab(tab_id)
                    .ok_or_else(|| err("tab_not_found", format!("tab {tab_id} not found")))?;
                if (tsi, tti) == (ssi, sti) {
                    return Ok(self.unchanged_move(&term, "same_tab"));
                }
                if self.spaces[tsi].tabs[tti].zoomed {
                    return Ok(self.unchanged_move(&term, "zoomed_tab"));
                }
                let target = match dest["target_pane_id"].as_str() {
                    Some(raw) => {
                        let target = self.resolve_pane(raw).ok_or_else(|| {
                            err(
                                "target_pane_not_found",
                                format!("target pane {raw} not found"),
                            )
                        })?;
                        if self.locate(&target) != Some((tsi, tti)) {
                            return Err(err(
                                "target_pane_not_found",
                                format!("target pane {raw} is not in tab {tab_id}"),
                            ));
                        }
                        target
                    }
                    None => self.spaces[tsi].tabs[tti].focused.clone(),
                };
                let split = match dest["split"].as_str() {
                    Some("down") => SplitDir::Down,
                    _ => SplitDir::Right,
                };
                let ratio = dest["ratio"].as_f64().unwrap_or(0.5);
                Resolved::Tab(self.tab_public(tsi, tti), target, split, ratio)
            }
            Some("new_tab") => {
                let ws = match dest["workspace_id"].as_str() {
                    Some(ws) => {
                        self.space_index(ws).ok_or_else(|| {
                            err("workspace_not_found", format!("workspace {ws} not found"))
                        })?;
                        ws.to_string()
                    }
                    None => self.spaces[ssi].id.clone(),
                };
                Resolved::NewTab(ws, dest["label"].as_str().map(str::to_string))
            }
            Some("new_workspace") => Resolved::NewSpace(
                dest["label"].as_str().map(str::to_string),
                dest["tab_label"].as_str().map(str::to_string),
            ),
            _ => return Err(err("invalid_params", "unknown destination")),
        };

        let index = self.moves;
        self.moves += 1;
        if self.faults.contains(&Fault::FailMove(index)) {
            return Err(err("pane_move_failed", "target pane could not be split"));
        }
        let lose_reply = self.faults.contains(&Fault::LoseMoveReply(index));

        let previous_pane_id = self.pane_public(&term).unwrap();
        let previous_workspace_id = self.spaces[ssi].id.clone();
        let previous_tab_id = self.tab_public(ssi, sti);

        // Take the pane out of its tab.
        let mut closed_tab = None;
        let remaining = tree_without(&self.spaces[ssi].tabs[sti].root, &term);
        match remaining {
            None => {
                self.spaces[ssi].tabs.remove(sti);
                State::adjust_after_tab_removal(&mut self.spaces[ssi], sti);
                closed_tab = Some(previous_tab_id.clone());
            }
            Some(tree) => {
                let tab = &mut self.spaces[ssi].tabs[sti];
                if tab.focused == term {
                    tab.focused = tree.first_leaf().to_string();
                }
                tab.root = tree;
            }
        }
        let source_space_empty = self.spaces[ssi].tabs.is_empty();
        let target_space_id = match &resolved {
            Resolved::Tab(tab_id, ..) => tab_id.split(":t").next().unwrap().to_string(),
            Resolved::NewTab(ws, _) => ws.clone(),
            Resolved::NewSpace(..) => String::new(),
        };
        let cross = target_space_id != previous_workspace_id;
        if cross {
            self.spaces[ssi].numbers.remove(&term);
            self.aliases.insert(previous_pane_id.clone(), term.clone());
        }
        let mut closed_space = None;
        if source_space_empty && cross {
            self.spaces.remove(ssi);
            closed_space = Some(previous_workspace_id.clone());
            if let Some(active) = self.active {
                if self.spaces.is_empty() {
                    self.active = None;
                } else if active == ssi {
                    self.active = Some(ssi.min(self.spaces.len() - 1));
                } else if active > ssi {
                    self.active = Some(active - 1);
                }
            }
        }

        let (tsi, tti, created_tab, created_space) = match resolved {
            Resolved::Tab(tab_id, target, split, ratio) => {
                let (tsi, tti) = self.resolve_tab(&tab_id).unwrap();
                let tab = &mut self.spaces[tsi].tabs[tti];
                tab.root = tree_insert(&tab.root, &target, &term, split, ratio);
                if focus {
                    tab.focused = term.clone();
                }
                (tsi, tti, false, false)
            }
            Resolved::NewTab(ws, label) => {
                let tsi = self.space_index(&ws).unwrap();
                let space = &mut self.spaces[tsi];
                let number = space.next_tab;
                space.next_tab += 1;
                space.tabs.push(Tab {
                    number,
                    custom: label,
                    root: Tree::leaf(&term),
                    focused: term.clone(),
                    zoomed: false,
                });
                (tsi, space.tabs.len() - 1, true, false)
            }
            Resolved::NewSpace(label, tab_label) => {
                self.next_space += 1;
                let cwd = self.terms[&term].pane.cwd.clone().unwrap_or_default();
                self.spaces.push(Space {
                    id: format!("w{}", self.next_space),
                    label: label.unwrap_or_else(|| basename(&cwd)),
                    tabs: vec![Tab {
                        number: 1,
                        custom: tab_label,
                        root: Tree::leaf(&term),
                        focused: term.clone(),
                        zoomed: false,
                    }],
                    active_tab: 0,
                    next_pane: 1,
                    next_tab: 2,
                    numbers: HashMap::new(),
                });
                (self.spaces.len() - 1, 0, true, true)
            }
        };
        self.register(tsi, &term);
        if focus || self.active.is_none() {
            self.active = Some(tsi);
            self.spaces[tsi].active_tab = tti;
            self.spaces[tsi].tabs[tti].focused = term.clone();
        }
        let reply = json!({"move_result": {
            "changed": true,
            "reason": null,
            "previous_pane_id": previous_pane_id,
            "previous_workspace_id": previous_workspace_id,
            "previous_tab_id": previous_tab_id,
            "pane": self.pane_json(&term),
            "target_layout": self.layout_json(tsi, tti),
            "created_tab": if created_tab { self.tab_json(tsi, tti) } else { Value::Null },
            "created_workspace": if created_space { self.space_json(tsi) } else { Value::Null },
            "closed_tab_id": closed_tab,
            "closed_workspace_id": closed_space,
            "focused_pane_id": self.pane_public(&self.spaces[tsi].tabs[tti].focused),
        }});
        if lose_reply {
            return Err(HerdrError::ReplyLost("simulated lost reply".into()));
        }
        Ok(reply)
    }

    fn dispatch(&mut self, method: &str, params: &Value) -> Result<Value, HerdrError> {
        match method {
            "ping" => Ok(json!({"type": "pong", "version": self.version, "protocol": 22})),
            "session.snapshot" => Ok(self.snapshot_json()),
            "pane.get" => {
                let id = params["pane_id"].as_str().unwrap_or_default();
                let term = self
                    .resolve_pane(id)
                    .ok_or_else(|| err("pane_not_found", "pane not found"))?;
                Ok(json!({"pane": self.pane_json(&term)}))
            }
            "pane.move" => self.pane_move(params),
            "tab.move" => {
                if self.faults.contains(&Fault::FailTabMove) {
                    return Err(err("tab_move_failed", "simulated tab.move failure"));
                }
                let tab_id = params["tab_id"].as_str().unwrap_or_default();
                let (si, ti) = self
                    .resolve_tab(tab_id)
                    .ok_or_else(|| err("tab_not_found", "tab not found"))?;
                let insert = params["insert_index"].as_u64().unwrap_or(0) as usize;
                let space = &mut self.spaces[si];
                if insert > space.tabs.len() {
                    return Err(err(
                        "tab_move_failed",
                        format!("insert_index {insert} is out of bounds"),
                    ));
                }
                let target =
                    if ti < insert { insert - 1 } else { insert }.min(space.tabs.len() - 1);
                if target != ti {
                    let active_number = space.tabs[space.active_tab].number;
                    let tab = space.tabs.remove(ti);
                    space.tabs.insert(target, tab);
                    space.active_tab = space
                        .tabs
                        .iter()
                        .position(|t| t.number == active_number)
                        .unwrap_or(target);
                }
                let tabs: Vec<Value> = (0..self.spaces[si].tabs.len())
                    .map(|t| self.tab_json(si, t))
                    .collect();
                Ok(json!({"tabs": tabs}))
            }
            "workspace.move" => {
                let id = params["workspace_id"].as_str().unwrap_or_default();
                let si = self
                    .space_index(id)
                    .ok_or_else(|| err("workspace_not_found", "workspace not found"))?;
                let insert = params["insert_index"].as_u64().unwrap_or(0) as usize;
                if insert > self.spaces.len() {
                    return Err(err("workspace_move_failed", "insert_index out of bounds"));
                }
                let target = if si < insert { insert - 1 } else { insert };
                if target != si {
                    let active_id = self.active.map(|a| self.spaces[a].id.clone());
                    let space = self.spaces.remove(si);
                    self.spaces.insert(target, space);
                    self.active = active_id.and_then(|id| self.space_index(&id));
                }
                Ok(
                    json!({"workspaces": (0..self.spaces.len()).map(|s| self.space_json(s)).collect::<Vec<_>>()}),
                )
            }
            "pane.zoom" => {
                let id = params["pane_id"].as_str().unwrap_or_default();
                let term = self
                    .resolve_pane(id)
                    .ok_or_else(|| err("pane_not_found", "pane not found"))?;
                let (si, ti) = self.locate(&term).unwrap();
                let on = params["mode"].as_str() == Some("on");
                let tab = &mut self.spaces[si].tabs[ti];
                let single = tab.root.leaves().len() == 1;
                let before = tab.zoomed;
                let mut reason = Value::Null;
                if on && single {
                    reason = json!("single_pane");
                } else if on {
                    tab.zoomed = true;
                    tab.focused = term.clone();
                } else {
                    if !before {
                        reason = json!("already_unzoomed");
                    }
                    tab.zoomed = false;
                }
                let changed = tab.zoomed != before;
                Ok(
                    json!({"zoom": {"changed": changed, "zoom_changed": changed, "focus_changed": false,
                    "reason": reason, "pane_id": id, "zoomed": tab.zoomed,
                    "focused_pane_id": self.pane_public(&self.spaces[si].tabs[ti].focused),
                    "layout": self.layout_json(si, ti)}}),
                )
            }
            "pane.swap" => {
                let a = self
                    .resolve_pane(params["source_pane_id"].as_str().unwrap_or_default())
                    .ok_or_else(|| err("pane_not_found", "pane not found"))?;
                let b = self
                    .resolve_pane(params["target_pane_id"].as_str().unwrap_or_default())
                    .ok_or_else(|| err("pane_not_found", "pane not found"))?;
                let (si, ti) = self.locate(&a).unwrap();
                if self.locate(&b) != Some((si, ti)) {
                    return Ok(json!({"swap": {"changed": false, "reason": "cross_tab"}}));
                }
                let map: HashMap<String, String> = [(a.clone(), b.clone()), (b.clone(), a.clone())]
                    .into_iter()
                    .collect();
                let tab = &mut self.spaces[si].tabs[ti];
                tab.root = tab.root.map_leaves(&map);
                Ok(json!({"swap": {"changed": true}}))
            }
            "pane.focus" => {
                let id = params["pane_id"].as_str().unwrap_or_default();
                let term = self
                    .resolve_pane(id)
                    .ok_or_else(|| err("pane_not_found", "pane not found"))?;
                let (si, ti) = self.locate(&term).unwrap();
                self.active = Some(si);
                self.spaces[si].active_tab = ti;
                self.spaces[si].tabs[ti].focused = term.clone();
                Ok(json!({"pane": self.pane_json(&term)}))
            }
            "layout.export" => {
                let tab_id = params["tab_id"].as_str().unwrap_or_default();
                let (si, ti) = self
                    .resolve_tab(tab_id)
                    .ok_or_else(|| err("tab_not_found", "tab not found"))?;
                let tab = &self.spaces[si].tabs[ti];
                Ok(json!({"layout": {
                    "workspace_id": self.spaces[si].id, "tab_id": tab_id, "zoomed": tab.zoomed,
                    "focused_pane_id": self.pane_public(&tab.focused),
                    "root": self.export_tree(&tab.root)}}))
            }
            "pane.process_info" => {
                let id = params["pane_id"].as_str().unwrap_or_default();
                let term = self
                    .resolve_pane(id)
                    .ok_or_else(|| err("pane_not_found", "pane not found"))?;
                let argv = self.terms[&term]
                    .pane
                    .command
                    .clone()
                    .unwrap_or_else(|| vec!["-zsh".into()]);
                Ok(json!({"process_info": {"pane_id": id, "shell_pid": 99,
                    "foreground_process_group_id": 100,
                    "foreground_processes": [{"pid": 100, "name": basename(argv[0].trim_start_matches('-')), "argv": argv}]}}))
            }
            "notification.show" => Ok(json!({"shown": self.toast.0, "reason": self.toast.1})),
            "plugin.pane.open" => {
                if self.popup_busy {
                    Err(err("ui_busy", "a popup pane is already open"))
                } else {
                    Ok(json!({}))
                }
            }
            other => Err(err(
                "unknown_method",
                format!("sim does not implement {other}"),
            )),
        }
    }
}

impl Sim {
    pub fn from_fixture(fixture: &Fixture) -> Self {
        let mut state = State {
            version: fixture.version.clone().unwrap_or_else(|| "0.9.0".into()),
            toast: (false, "disabled".into()),
            ..Default::default()
        };
        let mut active = None;
        for (si, fx_space) in fixture.spaces.iter().enumerate() {
            state.next_space += 1;
            let mut space = Space {
                id: format!("w{}", state.next_space),
                label: fx_space.label.clone(),
                tabs: Vec::new(),
                active_tab: 0,
                next_pane: 1,
                next_tab: 1,
                numbers: HashMap::new(),
            };
            for (ti, fx_tab) in fx_space.tabs.iter().enumerate() {
                let mut focused = None;
                let root = Self::build(
                    &mut state,
                    &mut space,
                    &fx_tab.layout,
                    &mut focused,
                    &mut active,
                    (si, ti),
                );
                let number = space.next_tab;
                space.next_tab += 1;
                let focused = focused.unwrap_or_else(|| root.first_leaf().to_string());
                space.tabs.push(Tab {
                    number,
                    custom: fx_tab.label.clone(),
                    root,
                    focused,
                    zoomed: fx_tab.zoomed,
                });
            }
            state.spaces.push(space);
        }
        match active {
            Some((si, ti)) => {
                state.active = Some(si);
                state.spaces[si].active_tab = ti;
            }
            None if !state.spaces.is_empty() => state.active = Some(0),
            None => {}
        }
        Self {
            state: Mutex::new(state),
        }
    }

    fn build(
        state: &mut State,
        space: &mut Space,
        node: &FxNode,
        focused: &mut Option<String>,
        active: &mut Option<(usize, usize)>,
        at: (usize, usize),
    ) -> Tree {
        match node {
            FxNode::Pane { pane } => {
                state.next_term += 1;
                let term = format!("term-{}", state.next_term);
                let handle = pane
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("p{}", state.next_term));
                space.numbers.insert(term.clone(), space.next_pane);
                space.next_pane += 1;
                if pane.focused || pane.active {
                    *focused = Some(term.clone());
                }
                if pane.active {
                    *active = Some(at);
                }
                state.handles.insert(handle.clone(), term.clone());
                state.terms.insert(
                    term.clone(),
                    Term {
                        handle,
                        pane: (**pane).clone(),
                    },
                );
                Tree::leaf(term)
            }
            FxNode::Split {
                split,
                ratio,
                first,
                second,
            } => Tree::split(
                *split,
                *ratio,
                Self::build(state, space, first, focused, active, at),
                Self::build(state, space, second, focused, active, at),
            ),
        }
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let fixture: Fixture = serde_json::from_str(text).map_err(|e| e.to_string())?;
        Ok(Self::from_fixture(&fixture))
    }

    fn with<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        f(&mut self.state.lock().unwrap_or_else(|p| p.into_inner()))
    }

    pub fn inject(&self, fault: Fault) {
        self.with(|s| s.faults.push(fault));
    }

    pub fn set_toast(&self, shown: bool, reason: &str) {
        self.with(|s| s.toast = (shown, reason.into()));
    }

    pub fn set_popup_busy(&self, busy: bool) {
        self.with(|s| s.popup_busy = busy);
    }

    /// Current public pane id of a fixture handle.
    pub fn pane_id(&self, handle: &str) -> String {
        self.with(|s| {
            let term = &s.handles[handle];
            s.pane_public(term)
                .unwrap_or_else(|| format!("<closed {handle}>"))
        })
    }

    pub fn terminal(&self, handle: &str) -> String {
        self.with(|s| s.handles[handle].clone())
    }

    /// Tab label and space label holding a handle.
    pub fn where_is(&self, handle: &str) -> Option<(String, String)> {
        self.with(|s| {
            let (si, ti) = s.locate(&s.handles[handle])?;
            Some((s.spaces[si].label.clone(), s.tab_label(si, ti)))
        })
    }

    pub fn tab_id_of(&self, handle: &str) -> String {
        self.with(|s| {
            let (si, ti) = s.locate(&s.handles[handle]).expect("pane is open");
            s.tab_public(si, ti)
        })
    }

    /// Tab labels of a space in bar order.
    pub fn tab_labels(&self, space: &str) -> Vec<String> {
        self.with(|s| {
            s.spaces
                .iter()
                .enumerate()
                .filter(|(_, sp)| sp.label == space)
                .flat_map(|(si, sp)| (0..sp.tabs.len()).map(move |ti| (si, ti)))
                .map(|(si, ti)| s.tab_label(si, ti))
                .collect()
        })
    }

    pub fn space_labels(&self) -> Vec<String> {
        self.with(|s| s.spaces.iter().map(|sp| sp.label.clone()).collect())
    }

    /// The split tree of the tab holding `handle`, with handles as leaves.
    pub fn tree_of(&self, handle: &str) -> Tree {
        self.with(|s| {
            let (si, ti) = s.locate(&s.handles[handle]).expect("pane is open");
            let map: HashMap<String, String> = s
                .terms
                .iter()
                .map(|(term, t)| (term.clone(), t.handle.clone()))
                .collect();
            s.spaces[si].tabs[ti].root.map_leaves(&map)
        })
    }

    pub fn is_zoomed(&self, handle: &str) -> bool {
        self.with(|s| {
            let (si, ti) = s.locate(&s.handles[handle]).expect("pane is open");
            s.spaces[si].tabs[ti].zoomed
        })
    }

    /// Handle of the session's focused pane.
    pub fn focused(&self) -> Option<String> {
        self.with(|s| {
            let si = s.active?;
            let space = &s.spaces[si];
            let term = &space.tabs.get(space.active_tab)?.focused;
            Some(s.terms[term].handle.clone())
        })
    }

    pub fn is_open(&self, handle: &str) -> bool {
        self.with(|s| s.locate(&s.handles[handle]).is_some())
    }

    /// Close a pane as if its process exited (edge cases 2.10, 5.11).
    pub fn close_pane(&self, handle: &str) {
        self.with(|s| {
            let term = s.handles[handle].clone();
            if let Some((si, ti)) = s.locate(&term) {
                match tree_without(&s.spaces[si].tabs[ti].root, &term) {
                    Some(tree) => {
                        let tab = &mut s.spaces[si].tabs[ti];
                        if tab.focused == term {
                            tab.focused = tree.first_leaf().to_string();
                        }
                        tab.root = tree;
                    }
                    None => {
                        s.spaces[si].tabs.remove(ti);
                        State::adjust_after_tab_removal(&mut s.spaces[si], ti);
                        if s.spaces[si].tabs.is_empty() {
                            s.spaces.remove(si);
                            s.active = if s.spaces.is_empty() { None } else { Some(0) };
                        }
                    }
                }
            }
        });
    }

    /// Requests received, filtered by method.
    pub fn calls(&self, method: &str) -> Vec<Value> {
        self.with(|s| {
            s.calls
                .iter()
                .filter(|(m, _)| m == method)
                .map(|(_, p)| p.clone())
                .collect()
        })
    }

    pub fn all_calls(&self) -> Vec<String> {
        self.with(|s| s.calls.iter().map(|(m, _)| m.clone()).collect())
    }
}

impl Herdr for Sim {
    fn request(&self, method: &str, params: Value) -> Result<Value, HerdrError> {
        self.with(|s| {
            s.calls.push((method.to_string(), params.clone()));
            s.dispatch(method, &params)
        })
    }

    fn session_key(&self) -> String {
        "sim".into()
    }
}

/// The sample session used by `panemorph preview` and the pty tests: the
/// spaces and tabs from the redesign's screens.
pub const SAMPLE_FIXTURE: &str = r#"{
  "spaces": [
    {"label": "Studio", "tabs": [
      {"label": "Landing_Page_Copy", "layout": {"split": "right",
        "first": {"pane": {"name": "headline", "agent": "claude", "label": "headline rewrite",
                           "cwd": "/Users/j/Desktop/Studio", "status": "done"}},
        "second": {"pane": {"name": "portfolio", "agent": "claude", "label": "portfolio copy",
                            "cwd": "/Users/j/Desktop/acme-web", "status": "idle", "active": true}}}},
      {"label": "API refactor", "layout": {"pane": {"name": "zsh", "cwd": "/Users/j/Desktop/acme-web"}}},
      {"label": "Load tests", "layout": {"pane": {"name": "scraper", "cwd": "/Users/j/Desktop/acme-web",
                                                   "command": ["python3", "scrape_docs.py"]}}}
    ]},
    {"label": "Payments_Service_Rewrite", "tabs": [
      {"label": "deploying", "layout": {"pane": {"name": "incident", "agent": "claude",
        "label": "rollback plan", "cwd": "/Users/j/code/incident-agent", "status": "working"}}}
    ]}
  ]
}"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api;

    fn sample() -> Sim {
        Sim::from_json(SAMPLE_FIXTURE).unwrap()
    }

    #[test]
    fn sample_snapshot_parses_into_the_model() {
        let sim = sample();
        let snapshot = api::snapshot(&sim).unwrap();
        assert_eq!(snapshot.workspaces.len(), 2);
        assert_eq!(snapshot.tabs.len(), 4);
        assert_eq!(
            snapshot.focused_pane_id.as_deref(),
            Some(sim.pane_id("portfolio").as_str())
        );
        assert_eq!(snapshot.tabs[0].label, "Landing_Page_Copy");
    }

    #[test]
    fn cross_space_move_gets_new_id_and_keeps_alias() {
        let sim = sample();
        let old = sim.pane_id("scraper");
        let incident_tab = sim.tab_id_of("incident");
        let dest = api::MoveDest::Tab {
            tab_id: incident_tab,
            target_pane_id: None,
            split: SplitDir::Right,
            ratio: 0.5,
        };
        let result = api::pane_move(&sim, &old, &dest, false).unwrap();
        assert!(result.changed);
        assert_ne!(result.pane.pane_id, old);
        assert!(result.closed_tab_id.is_some());
        // The old id resolves through the alias.
        assert_eq!(
            api::pane_get(&sim, &old).unwrap().pane_id,
            result.pane.pane_id
        );
    }

    #[test]
    fn zoomed_source_is_refused_like_herdr() {
        let sim = sample();
        api::pane_zoom(&sim, &sim.pane_id("portfolio"), true).unwrap();
        let result = api::pane_move(
            &sim,
            &sim.pane_id("portfolio"),
            &api::MoveDest::NewTab {
                workspace_id: "w1".into(),
                label: None,
            },
            false,
        )
        .unwrap();
        assert!(!result.changed);
        assert_eq!(result.reason.as_deref(), Some("zoomed_tab"));
    }

    #[test]
    fn last_pane_in_space_closes_the_space_on_cross_space_move() {
        let sim = sample();
        let result = api::pane_move(
            &sim,
            &sim.pane_id("incident"),
            &api::MoveDest::NewTab {
                workspace_id: "w1".into(),
                label: Some("x".into()),
            },
            false,
        )
        .unwrap();
        assert_eq!(result.closed_workspace_id.as_deref(), Some("w2"));
        assert_eq!(sim.space_labels(), ["Studio"]);
    }
}
