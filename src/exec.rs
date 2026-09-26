//! Carry out moves against herdr: the live `pane.move` calls plus the
//! unzooming, placement, focus and bookkeeping around them.
//!
//! paneMorph never destroys a terminal (edge case 7.11): the only mutating
//! calls here are `pane.move`, `pane.zoom`, `pane.swap`, `pane.focus`,
//! `tab.move` and `workspace.move`.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::api::{self, Herdr, HerdrError, MoveDest};
use crate::journal::{Entry, Journal, Place};
use crate::model::{Command, PaneInfo, Snapshot, SplitDir};
use crate::names;
use crate::plan::{self, FetchTarget, NoOp, SendTarget};
use crate::state::now_ms;
use crate::topology::{self, Tree};

/// A completed move.
#[derive(Debug, Clone, PartialEq)]
pub struct Done {
    pub summary: String,
    pub entry: Option<Box<Entry>>,
    /// Problems after the pane already moved (edge cases 3.7, 6.20).
    pub warnings: Vec<String>,
    /// One log line (edge case 7.9).
    pub log: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Done(Done),
    /// An edge press that does nothing (4.1, 4.5 …): toast only.
    NoOp(NoOp),
    /// Nothing moved, but the user should see why (5.11, 5.12).
    Notice(String),
}

/// A move that failed. `entry` is set when panes were left moved and the
/// journal should keep them so ⌃⌥Z can retry (edge case 7.2).
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub message: String,
    pub entry: Option<Box<Entry>>,
}

impl Failure {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            entry: None,
        }
    }
}

impl From<HerdrError> for Failure {
    fn from(error: HerdrError) -> Self {
        Failure::new(describe(&error))
    }
}

pub type ExecResult = Result<Outcome, Failure>;

/// Plain words for herdr errors (edge cases 2.10, 6.5).
pub fn describe(error: &HerdrError) -> String {
    match error.code() {
        Some("pane_not_found") | Some("target_pane_not_found") => plan::PANE_CLOSED.into(),
        Some("tab_not_found") => plan::TAB_CLOSED.into(),
        Some("workspace_not_found") => plan::SPACE_CLOSED.into(),
        _ => error.to_string(),
    }
}

/// Plain words for `changed: false` (edge case 7.4).
fn unchanged(reason: Option<&str>) -> Failure {
    Failure::new(match reason {
        Some("same_tab") => "Nothing moved: already in that tab",
        Some("zoomed_tab") => "Nothing moved: that tab is zoomed",
        _ => "Nothing moved",
    })
}

/// What one `pane.move` did.
#[derive(Debug, Clone)]
struct Moved {
    pane: PaneInfo,
    closed_tab: bool,
    closed_space: bool,
    created_tab: Option<String>,
    created_space: Option<String>,
}

pub struct Exec<'a> {
    herdr: &'a dyn Herdr,
}

fn who(pane: &PaneInfo, command: Option<&Command>) -> String {
    let words = names::pane_words(pane, command);
    if words.who == "shell" && !words.folder.is_empty() {
        format!("shell in {}", words.folder)
    } else {
        words.who
    }
}

impl<'a> Exec<'a> {
    pub fn new(herdr: &'a dyn Herdr) -> Self {
        Self { herdr }
    }

    fn snapshot(&self) -> Result<Snapshot, Failure> {
        api::snapshot(self.herdr).map_err(Failure::from)
    }

    /// Find a pane by id, following herdr's aliases (edge case 1.18).
    pub fn resolve(&self, snapshot: &Snapshot, pane_id: &str) -> Option<PaneInfo> {
        if let Some(pane) = snapshot.pane(pane_id) {
            return Some(pane.clone());
        }
        let found = api::pane_get(self.herdr, pane_id).ok()?;
        snapshot
            .pane_by_terminal(&found.terminal_id)
            .cloned()
            .or(Some(found))
    }

    /// The running command of a pane without an agent (edge case 2.12).
    pub fn command_for(&self, pane: &PaneInfo) -> Option<Command> {
        if names::agent(pane).is_some() {
            return None;
        }
        api::process_info(self.herdr, &pane.pane_id)
            .ok()
            .and_then(|info| info.command())
    }

    fn place(snapshot: &Snapshot, pane: &PaneInfo) -> Place {
        let tab = snapshot.tab(&pane.tab_id);
        let space = snapshot.workspace(&pane.workspace_id);
        Place {
            workspace_id: pane.workspace_id.clone(),
            workspace_label: space.map(|s| s.label.clone()).unwrap_or_default(),
            workspace_index: snapshot.workspace_index(&pane.workspace_id).unwrap_or(0),
            tab_id: pane.tab_id.clone(),
            tab_label: tab.and_then(|t| names::tab_custom_label(snapshot, t)),
            tab_index: snapshot.tab_index(&pane.tab_id).unwrap_or(0),
        }
    }

    /// The tab's split tree with terminal ids as leaves.
    fn terminal_tree(&self, snapshot: &Snapshot, tab_id: &str) -> Option<(Tree, Tree)> {
        let layout = api::layout_export(self.herdr, tab_id).ok()?;
        let by_pane = Tree::from_layout(&layout.root).ok()?;
        let map: HashMap<String, String> = snapshot
            .panes
            .iter()
            .map(|p| (p.pane_id.clone(), p.terminal_id.clone()))
            .collect();
        let by_terminal = by_pane.map_leaves(&map);
        Some((by_pane, by_terminal))
    }

    /// Unzoom a tab. Returns the terminal that was zoomed.
    fn unzoom(&self, snapshot: &Snapshot, tab_id: &str) -> Result<Option<String>, Failure> {
        let Some(layout) = snapshot.layout(tab_id) else {
            return Ok(None);
        };
        if !layout.zoomed {
            return Ok(None);
        }
        api::pane_zoom(self.herdr, &layout.focused_pane_id, false)?;
        Ok(snapshot
            .pane(&layout.focused_pane_id)
            .map(|p| p.terminal_id.clone()))
    }

    /// Zoom a pane again, found by terminal id.
    fn rezoom(&self, terminal: &str) -> Result<(), String> {
        let snapshot = api::snapshot(self.herdr).map_err(|e| e.to_string())?;
        let Some(pane) = snapshot.pane_by_terminal(terminal) else {
            return Ok(());
        };
        api::pane_zoom(self.herdr, &pane.pane_id, true)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// herdr 0.9.0 does not move an attached client on `pane.move` with
    /// `focus: true` (issue #4153); an explicit `pane.focus` does. Harmless
    /// on 0.9.1+, where the move already moved the client.
    fn follow(&self, pane_id: &str) {
        let _ = api::pane_focus(self.herdr, pane_id);
    }

    fn do_move(
        &self,
        before: &Snapshot,
        pane: &PaneInfo,
        dest: &MoveDest,
        focus: bool,
    ) -> Result<Moved, Failure> {
        match api::pane_move(self.herdr, &pane.pane_id, dest, focus) {
            Ok(result) if result.changed => Ok(Moved {
                pane: result.pane,
                closed_tab: result.closed_tab_id.is_some(),
                closed_space: result.closed_workspace_id.is_some(),
                created_tab: result.created_tab.map(|t| t.tab_id),
                created_space: result.created_workspace.map(|w| w.workspace_id),
            }),
            Ok(result) => Err(unchanged(result.reason.as_deref())),
            Err(HerdrError::ReplyLost(detail)) => self.verify_lost(before, pane, &detail),
            Err(error) => Err(Failure::from(error)),
        }
    }

    /// A move whose reply was lost: look the pane up by terminal id before
    /// reporting anything (edge case 6.6).
    fn verify_lost(
        &self,
        before: &Snapshot,
        pane: &PaneInfo,
        detail: &str,
    ) -> Result<Moved, Failure> {
        let after = self.snapshot()?;
        match after.pane_by_terminal(&pane.terminal_id) {
            None => Err(Failure::new(plan::PANE_CLOSED)),
            Some(now) if now.tab_id != pane.tab_id => Ok(Moved {
                pane: now.clone(),
                closed_tab: after.tab(&pane.tab_id).is_none(),
                closed_space: after.workspace(&pane.workspace_id).is_none(),
                created_tab: before
                    .tab(&now.tab_id)
                    .is_none()
                    .then(|| now.tab_id.clone()),
                created_space: before
                    .workspace(&now.workspace_id)
                    .is_none()
                    .then(|| now.workspace_id.clone()),
            }),
            Some(_) => Err(Failure::new(format!(
                "herdr did not answer and the pane did not move ({detail})"
            ))),
        }
    }

    // -----------------------------------------------------------------
    // Send and the quick keys.

    /// Send `source_id` to `target`. Focus goes with the pane (1.15).
    pub fn send(
        &self,
        source_id: &str,
        target: &SendTarget,
        split: SplitDir,
        action: &str,
    ) -> ExecResult {
        let started = Instant::now();
        let snapshot = self.snapshot()?;
        let pane = self
            .resolve(&snapshot, source_id)
            .ok_or_else(|| Failure::new(plan::PANE_CLOSED))?;
        if let Err(no_op) = plan::check_send(&snapshot, &pane, target) {
            return Ok(Outcome::NoOp(no_op));
        }
        let source = Self::place(&snapshot, &pane);
        let source_tree = self.terminal_tree(&snapshot, &pane.tab_id).map(|(_, t)| t);
        let command = self.command_for(&pane);
        let label = who(&pane, command.as_ref());

        let source_zoom = self.unzoom(&snapshot, &pane.tab_id)?;
        let mut target_zoom = None;
        let (dest, where_to, place_after) = match target {
            SendTarget::Tab(tab_id) => {
                target_zoom = self.unzoom(&snapshot, tab_id)?;
                let tab = snapshot
                    .tab(tab_id)
                    .map(names::tab_label)
                    .unwrap_or_default();
                (
                    MoveDest::Tab {
                        tab_id: tab_id.clone(),
                        target_pane_id: None,
                        split,
                        ratio: 0.5,
                    },
                    tab,
                    None,
                )
            }
            SendTarget::NewTabHere => {
                let name = names::unique_tab_name(
                    &snapshot,
                    &pane.workspace_id,
                    &names::tab_base_name(&pane, command.as_ref()),
                );
                (
                    MoveDest::NewTab {
                        workspace_id: pane.workspace_id.clone(),
                        label: Some(name.clone()),
                    },
                    format!("new tab {name}"),
                    snapshot.tab_index(&pane.tab_id),
                )
            }
            SendTarget::NewTabIn(workspace_id) => {
                let name = names::unique_tab_name(
                    &snapshot,
                    workspace_id,
                    &names::tab_base_name(&pane, command.as_ref()),
                );
                let space = snapshot
                    .workspace(workspace_id)
                    .map(|s| names::space_label(&snapshot, s))
                    .unwrap_or_default();
                (
                    MoveDest::NewTab {
                        workspace_id: workspace_id.clone(),
                        label: Some(name.clone()),
                    },
                    format!("new tab {name} in {space}"),
                    None,
                )
            }
            SendTarget::NewSpace => {
                let name = names::unique_space_name(
                    &snapshot,
                    &names::space_base_name(&pane, command.as_ref()),
                );
                (
                    MoveDest::NewSpace {
                        label: Some(name.clone()),
                        tab_label: Some(names::tab_base_name(&pane, command.as_ref())),
                    },
                    format!("new space {name}"),
                    None,
                )
            }
        };

        let moved = match self.do_move(&snapshot, &pane, &dest, true) {
            Ok(moved) => moved,
            Err(failure) => {
                // Nothing moved: put back any zoom paneMorph removed.
                for term in [source_zoom, target_zoom].into_iter().flatten() {
                    let _ = self.rezoom(&term);
                }
                return Err(failure);
            }
        };
        self.follow(&moved.pane.pane_id);

        let mut warnings = Vec::new();
        if let (Some(index), Some(new_tab)) = (place_after, moved.created_tab.as_ref()) {
            // herdr appended the tab at the end; the source tab kept its slot
            // because the pane was not alone in it (edge cases 1.2, 3.6).
            let last = snapshot.tabs_in(&pane.workspace_id).len();
            if index + 1 < last && api::tab_move(self.herdr, new_tab, index + 1).is_err() {
                warnings
                    .push("Moved, but couldn't place the new tab next to this one.".to_string());
            }
        }
        let mut rezoom_terminal = None;
        if let Some(term) = source_zoom {
            if term == pane.terminal_id {
                rezoom_terminal = Some(term);
            } else if !moved.closed_tab {
                if let Err(error) = self.rezoom(&term) {
                    warnings.push(format!(
                        "Moved, but couldn't zoom the other pane again: {error}"
                    ));
                }
            }
        }
        let entry = Entry {
            action: action.to_string(),
            at_ms: now_ms(),
            terminals: vec![pane.terminal_id.clone()],
            pane_ids_after: vec![moved.pane.pane_id.clone()],
            dest_tab_id: moved.pane.tab_id.clone(),
            source,
            source_tree,
            source_tab_closed: moved.closed_tab,
            source_space_closed: moved.closed_space,
            rezoom_terminal,
            followed: true,
            whole_tab: false,
        };
        let log = format!(
            "{action} terminal={} pane={}->{} from={} to={} result=changed{}{} {}ms",
            pane.terminal_id,
            pane.pane_id,
            moved.pane.pane_id,
            pane.tab_id,
            moved.pane.tab_id,
            if moved.closed_tab { " closed_tab" } else { "" },
            if moved.closed_space {
                " closed_space"
            } else {
                ""
            },
            started.elapsed().as_millis()
        );
        Ok(Outcome::Done(Done {
            summary: format!("Sent {label} to {where_to}"),
            entry: Some(Box::new(entry)),
            warnings,
            log,
        }))
    }

    // -----------------------------------------------------------------
    // Fetch.

    /// Bring `target` beside `dest_id`; you stay on your pane (2.2).
    pub fn fetch(
        &self,
        dest_id: &str,
        target: &FetchTarget,
        split: SplitDir,
        action: &str,
    ) -> ExecResult {
        let snapshot = self.snapshot()?;
        let you = self
            .resolve(&snapshot, dest_id)
            .ok_or_else(|| Failure::new("Your pane closed"))?;
        if let Err(no_op) = plan::check_fetch(&snapshot, &you, target) {
            return Ok(Outcome::NoOp(no_op));
        }
        match target {
            FetchTarget::Pane(pane_id) => self.fetch_pane(&snapshot, &you, pane_id, split, action),
            FetchTarget::Tab(tab_id) => self.fetch_tab(&snapshot, &you, tab_id, split, action),
        }
    }

    fn fetch_pane(
        &self,
        snapshot: &Snapshot,
        you: &PaneInfo,
        pane_id: &str,
        split: SplitDir,
        action: &str,
    ) -> ExecResult {
        let started = Instant::now();
        let pane = snapshot
            .pane(pane_id)
            .cloned()
            .ok_or_else(|| Failure::new(plan::PANE_CLOSED))?;
        let source = Self::place(snapshot, &pane);
        let source_tree = self.terminal_tree(snapshot, &pane.tab_id).map(|(_, t)| t);
        let command = self.command_for(&pane);
        let label = who(&pane, command.as_ref());
        let source_zoom = self.unzoom(snapshot, &pane.tab_id)?;
        let dest_zoom = self.unzoom(snapshot, &you.tab_id)?;
        let dest = MoveDest::Tab {
            tab_id: you.tab_id.clone(),
            target_pane_id: Some(you.pane_id.clone()),
            split,
            ratio: 0.5,
        };
        let moved = match self.do_move(snapshot, &pane, &dest, false) {
            Ok(moved) => moved,
            Err(failure) => {
                for term in [source_zoom, dest_zoom].into_iter().flatten() {
                    let _ = self.rezoom(&term);
                }
                return Err(failure);
            }
        };
        let mut warnings = Vec::new();
        let mut rezoom_terminal = None;
        if let Some(term) = source_zoom {
            if term == pane.terminal_id {
                rezoom_terminal = Some(term);
            } else if !moved.closed_tab {
                // Edge case 2.9: zoom the other tab's pane again.
                if let Err(error) = self.rezoom(&term) {
                    warnings.push(format!(
                        "Fetched, but couldn't zoom that tab again: {error}"
                    ));
                }
            }
        }
        let entry = Entry {
            action: action.to_string(),
            at_ms: now_ms(),
            terminals: vec![pane.terminal_id.clone()],
            pane_ids_after: vec![moved.pane.pane_id.clone()],
            dest_tab_id: you.tab_id.clone(),
            source,
            source_tree,
            source_tab_closed: moved.closed_tab,
            source_space_closed: moved.closed_space,
            rezoom_terminal,
            followed: false,
            whole_tab: false,
        };
        let log = format!(
            "{action} terminal={} pane={}->{} from={} to={} result=changed{}{} {}ms",
            pane.terminal_id,
            pane.pane_id,
            moved.pane.pane_id,
            pane.tab_id,
            you.tab_id,
            if moved.closed_tab { " closed_tab" } else { "" },
            if moved.closed_space {
                " closed_space"
            } else {
                ""
            },
            started.elapsed().as_millis()
        );
        Ok(Outcome::Done(Done {
            summary: format!("Fetched {label}"),
            entry: Some(Box::new(entry)),
            warnings,
            log,
        }))
    }

    /// Fetch every pane of `tab_id`, rebuilding its splits (2.4, 2.6, 2.16).
    fn fetch_tab(
        &self,
        snapshot: &Snapshot,
        you: &PaneInfo,
        tab_id: &str,
        split: SplitDir,
        action: &str,
    ) -> ExecResult {
        let started = Instant::now();
        let tab = snapshot
            .tab(tab_id)
            .cloned()
            .ok_or_else(|| Failure::new(plan::TAB_CLOSED))?;
        let first = snapshot
            .panes_in_tab(tab_id)
            .first()
            .map(|p| (*p).clone())
            .ok_or_else(|| Failure::new(plan::TAB_CLOSED))?;
        let source = Self::place(snapshot, &first);
        let (by_pane, by_terminal) = self
            .terminal_tree(snapshot, tab_id)
            .ok_or_else(|| Failure::new("herdr could not export that tab's layout"))?;
        let source_zoom = self.unzoom(snapshot, tab_id)?;
        let _ = self.unzoom(snapshot, &you.tab_id)?;
        let plan = topology::plan_tab_merge(&by_pane, &you.tab_id, &you.pane_id, split, 0.5);
        let terminal_of: HashMap<String, String> = snapshot
            .panes
            .iter()
            .map(|p| (p.pane_id.clone(), p.terminal_id.clone()))
            .collect();
        let mut id_map: HashMap<String, String> = HashMap::new();
        let mut moved_terms: Vec<String> = Vec::new();
        let mut moved_ids: Vec<String> = Vec::new();
        let mut last: Option<Moved> = None;
        for step in &plan {
            let pane_id = id_map
                .get(&step.pane_id)
                .cloned()
                .unwrap_or_else(|| step.pane_id.clone());
            let target = id_map
                .get(&step.target_pane_id)
                .cloned()
                .unwrap_or_else(|| step.target_pane_id.clone());
            let current = self.snapshot()?;
            let Some(pane) = current.pane(&pane_id).cloned() else {
                let failure = Failure::new(plan::PANE_CLOSED);
                return Err(self.whole_tab_failure(
                    failure,
                    &source,
                    &by_terminal,
                    &moved_terms,
                    you,
                    action,
                ));
            };
            let dest = MoveDest::Tab {
                tab_id: you.tab_id.clone(),
                target_pane_id: Some(target),
                split: step.split,
                ratio: step.ratio,
            };
            match self.do_move(&current, &pane, &dest, false) {
                Ok(moved) => {
                    id_map.insert(step.pane_id.clone(), moved.pane.pane_id.clone());
                    moved_terms.push(
                        terminal_of
                            .get(&step.pane_id)
                            .cloned()
                            .unwrap_or_else(|| moved.pane.terminal_id.clone()),
                    );
                    moved_ids.push(moved.pane.pane_id.clone());
                    last = Some(moved);
                }
                Err(failure) => {
                    let mut to_return = moved_terms.clone();
                    // herdr may have recovered the failed pane into a new
                    // tab; bring it back too (edge case 7.3).
                    to_return.push(pane.terminal_id.clone());
                    return Err(self.whole_tab_failure(
                        failure,
                        &source,
                        &by_terminal,
                        &to_return,
                        you,
                        action,
                    ));
                }
            }
        }
        let last = last.expect("a plan always has one step");
        let entry = Entry {
            action: action.to_string(),
            at_ms: now_ms(),
            terminals: moved_terms.clone(),
            pane_ids_after: moved_ids,
            dest_tab_id: you.tab_id.clone(),
            source,
            source_tree: Some(by_terminal),
            source_tab_closed: last.closed_tab,
            source_space_closed: last.closed_space,
            rezoom_terminal: source_zoom,
            followed: false,
            whole_tab: true,
        };
        let count = moved_terms.len();
        let log = format!(
            "{action} tab={tab_id} panes={count} to={} result=changed{}{} {}ms",
            you.tab_id,
            if last.closed_tab { " closed_tab" } else { "" },
            if last.closed_space {
                " closed_space"
            } else {
                ""
            },
            started.elapsed().as_millis()
        );
        Ok(Outcome::Done(Done {
            summary: format!(
                "Fetched {} ({count} pane{})",
                names::tab_label(&tab),
                if count == 1 { "" } else { "s" }
            ),
            entry: Some(Box::new(entry)),
            warnings: Vec::new(),
            log,
        }))
    }

    /// Put back the panes a failed whole-tab fetch had moved (7.1–7.3).
    fn whole_tab_failure(
        &self,
        cause: Failure,
        source: &Place,
        tree: &Tree,
        moved: &[String],
        you: &PaneInfo,
        action: &str,
    ) -> Failure {
        match self.return_panes(source, tree, moved) {
            Ok(()) => Failure::new(format!(
                "Couldn't fetch the whole tab; nothing changed. ({})",
                cause.message
            )),
            Err(error) => {
                // Edge case 7.2: leave everything where it is and say where.
                let snapshot = api::snapshot(self.herdr).unwrap_or_default();
                let mut places = Vec::new();
                let mut still_here = Vec::new();
                for term in moved {
                    if let Some(pane) = snapshot.pane_by_terminal(term) {
                        let tab = snapshot
                            .tab(&pane.tab_id)
                            .map(names::tab_label)
                            .unwrap_or_default();
                        places.push(format!("{} in {tab}", who(pane, None)));
                        if pane.tab_id == you.tab_id {
                            still_here.push(term.clone());
                        }
                    }
                }
                let entry = (!still_here.is_empty()).then(|| {
                    Box::new(Entry {
                        action: action.to_string(),
                        at_ms: now_ms(),
                        terminals: still_here,
                        dest_tab_id: you.tab_id.clone(),
                        source: source.clone(),
                        source_tree: Some(tree.clone()),
                        whole_tab: true,
                        ..Default::default()
                    })
                });
                Failure {
                    message: format!(
                        "Couldn't fetch the whole tab or put it back ({}; {error}). Every process is still running: {}. ⌃⌥Z retries.",
                        cause.message,
                        places.join(", ")
                    ),
                    entry,
                }
            }
        }
    }

    /// Return `terminals` to the source tab, each beside its recorded
    /// neighbour, in the order that keeps the geometry exact.
    fn return_panes(
        &self,
        source: &Place,
        tree: &Tree,
        terminals: &[String],
    ) -> Result<(), String> {
        let snapshot = api::snapshot(self.herdr).map_err(|e| e.to_string())?;
        if snapshot.tab(&source.tab_id).is_none() {
            return Err("the source tab closed".into());
        }
        let mut present: HashSet<String> = snapshot
            .panes
            .iter()
            .filter(|p| p.tab_id == source.tab_id)
            .map(|p| p.terminal_id.clone())
            .collect();
        let missing: Vec<String> = terminals
            .iter()
            .filter(|t| !present.contains(*t) && snapshot.pane_by_terminal(t).is_some())
            .cloned()
            .collect();
        for term in topology::return_order(tree, &present, &missing) {
            self.return_one(&term, &source.tab_id, Some(tree), &mut present, false)
                .map_err(|f| f.message)?;
        }
        Ok(())
    }

    /// Move one terminal into `tab_id` beside its recorded neighbour (5.5),
    /// or right of the tab's focused pane when no neighbour is left (5.6).
    fn return_one(
        &self,
        term: &str,
        tab_id: &str,
        tree: Option<&Tree>,
        present: &mut HashSet<String>,
        focus: bool,
    ) -> Result<PaneInfo, Failure> {
        let snapshot = self.snapshot()?;
        let pane = snapshot
            .pane_by_terminal(term)
            .cloned()
            .ok_or_else(|| Failure::new(plan::PANE_CLOSED))?;
        let placement = tree
            .and_then(|t| topology::reinsertion(t, present, term))
            .and_then(|p| {
                let anchor = snapshot.pane_by_terminal(&p.anchor)?.clone();
                (anchor.tab_id == tab_id).then_some((p, anchor))
            });
        let moved = match &placement {
            Some((p, anchor)) => self.do_move(
                &snapshot,
                &pane,
                &MoveDest::Tab {
                    tab_id: tab_id.to_string(),
                    target_pane_id: Some(anchor.pane_id.clone()),
                    split: p.split,
                    ratio: p.ratio,
                },
                focus,
            )?,
            None => self.do_move(
                &snapshot,
                &pane,
                &MoveDest::Tab {
                    tab_id: tab_id.to_string(),
                    target_pane_id: None,
                    split: SplitDir::Right,
                    ratio: 0.5,
                },
                focus,
            )?,
        };
        if let Some((p, anchor)) = &placement {
            if p.swap {
                api::pane_swap(self.herdr, &moved.pane.pane_id, &anchor.pane_id)?;
            }
        }
        present.insert(term.to_string());
        Ok(moved.pane)
    }

    // -----------------------------------------------------------------
    // Undo.

    /// Undo the newest journal entry (edge cases 5.3–5.18). The caller
    /// saves the journal afterwards.
    pub fn undo(&self, journal: &mut Journal, focused_pane_id: Option<&str>) -> ExecResult {
        let started = Instant::now();
        let Some(entry) = journal.last().cloned() else {
            return Ok(Outcome::NoOp(NoOp::new(plan::NOTHING_TO_UNDO)));
        };
        let snapshot = self.snapshot()?;
        // Edge case 5.13: after a herdr restart no journaled terminal exists.
        let any_alive = journal
            .entries
            .iter()
            .flat_map(|e| e.terminals.iter())
            .any(|t| snapshot.pane_by_terminal(t).is_some());
        if !any_alive {
            journal.clear();
            return Ok(Outcome::NoOp(NoOp::new(plan::NOTHING_TO_UNDO)));
        }
        let panes: Vec<Option<PaneInfo>> = entry
            .terminals
            .iter()
            .map(|t| snapshot.pane_by_terminal(t).cloned())
            .collect();
        if panes.iter().any(Option::is_none) {
            // Edge case 5.11.
            journal.pop();
            return Ok(Outcome::Notice("Can't undo: that pane was closed".into()));
        }
        let panes: Vec<PaneInfo> = panes.into_iter().flatten().collect();
        if panes.iter().any(|p| p.tab_id != entry.dest_tab_id) {
            // Edge case 5.12.
            journal.pop();
            return Ok(Outcome::Notice(
                "Can't undo: that pane was moved since".into(),
            ));
        }
        // Edge case 5.4: go back with the pane only if you are on it.
        let focused_terminal = focused_pane_id
            .and_then(|id| self.resolve(&snapshot, id))
            .map(|p| p.terminal_id);
        let followed = focused_terminal
            .as_ref()
            .is_some_and(|t| entry.terminals.contains(t));

        let dest_zoom = self.unzoom(&snapshot, &entry.dest_tab_id)?;
        let tree = entry.source_tree.as_ref();
        let mut returned: Vec<PaneInfo> = Vec::new();
        let mut warnings = Vec::new();
        let back_to;
        if snapshot.tab(&entry.source.tab_id).is_some() {
            self.unzoom(&snapshot, &entry.source.tab_id)?;
            let mut present: HashSet<String> = snapshot
                .panes
                .iter()
                .filter(|p| p.tab_id == entry.source.tab_id)
                .map(|p| p.terminal_id.clone())
                .collect();
            let order = match tree {
                Some(tree) => topology::return_order(tree, &present, &entry.terminals),
                None => entry.terminals.clone(),
            };
            for (index, term) in order.iter().enumerate() {
                returned.push(self.return_one(
                    term,
                    &entry.source.tab_id,
                    tree,
                    &mut present,
                    followed && index == 0,
                )?);
            }
            back_to = snapshot
                .tab(&entry.source.tab_id)
                .map(names::tab_label)
                .unwrap_or_default();
        } else {
            // The source tab (5.7) or space (5.8) closed: recreate it.
            let first = &panes[0];
            let space_alive = snapshot.workspace(&entry.source.workspace_id).is_some();
            let dest = if space_alive {
                MoveDest::NewTab {
                    workspace_id: entry.source.workspace_id.clone(),
                    label: entry.source.tab_label.clone(),
                }
            } else {
                MoveDest::NewSpace {
                    label: Some(entry.source.workspace_label.clone()).filter(|l| !l.is_empty()),
                    tab_label: entry.source.tab_label.clone(),
                }
            };
            let moved = self.do_move(&snapshot, first, &dest, followed)?;
            if space_alive {
                let count = snapshot.tabs_in(&entry.source.workspace_id).len() + 1;
                if entry.source.tab_index + 1 < count {
                    if let Some(tab) = &moved.created_tab {
                        if api::tab_move(self.herdr, tab, entry.source.tab_index).is_err() {
                            warnings.push(
                                "Undone, but couldn't put the tab back in its old place."
                                    .to_string(),
                            );
                        }
                    }
                }
            } else if let Some(space) = &moved.created_space {
                let count = snapshot.workspaces.len() + 1;
                if entry.source.workspace_index + 1 < count
                    && api::workspace_move(self.herdr, space, entry.source.workspace_index).is_err()
                {
                    warnings.push(
                        "Undone, but couldn't put the space back in its old place.".to_string(),
                    );
                }
            }
            let new_tab = moved.pane.tab_id.clone();
            journal.rename_tab(&entry.source.tab_id, &new_tab);
            if !space_alive {
                journal.rename_space(&entry.source.workspace_id, &moved.pane.workspace_id);
            }
            back_to = if space_alive {
                entry
                    .source
                    .tab_label
                    .clone()
                    .unwrap_or_else(|| "its tab".into())
            } else {
                entry.source.workspace_label.clone()
            };
            let mut present: HashSet<String> = [first.terminal_id.clone()].into_iter().collect();
            returned.push(moved.pane);
            let rest: Vec<String> = entry.terminals[1..].to_vec();
            let order = match tree {
                Some(tree) => topology::return_order(tree, &present, &rest),
                None => rest,
            };
            for term in order {
                returned.push(self.return_one(&term, &new_tab, tree, &mut present, false)?);
            }
        }
        if followed {
            if let Some(pane) = returned.first() {
                self.follow(&pane.pane_id);
            }
        }
        if let Some(term) = &entry.rezoom_terminal {
            if let Err(error) = self.rezoom(term) {
                warnings.push(format!("Undone, but couldn't zoom the pane again: {error}"));
            }
        }
        if let Some(term) = dest_zoom {
            if !entry.terminals.contains(&term) {
                let _ = self.rezoom(&term);
            }
        }
        journal.pop();
        let label = returned.first().map(|p| who(p, None)).unwrap_or_default();
        let log = format!(
            "undo {} terminals={} to={} {}ms",
            entry.action,
            entry.terminals.join(","),
            returned.first().map(|p| p.tab_id.as_str()).unwrap_or(""),
            started.elapsed().as_millis()
        );
        Ok(Outcome::Done(Done {
            summary: if entry.whole_tab {
                format!("Put {back_to} back")
            } else {
                format!("Put {label} back in {back_to}")
            },
            entry: None,
            warnings,
            log,
        }))
    }
}
