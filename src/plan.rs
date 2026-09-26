//! Pure decisions about what a move would do, from one snapshot.

use crate::model::{PaneInfo, Snapshot};
use crate::names;

/// Where Send puts the focused pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendTarget {
    /// An existing tab, in this space or another (1.1, 1.3).
    Tab(String),
    /// A new tab right after the current one (1.2, ⌃⌥T).
    NewTabHere,
    /// A new tab at the end of another space (1.4).
    NewTabIn(String),
    /// A new space at the bottom of the sidebar (1.5, ⌃⌥N).
    NewSpace,
}

/// What Fetch brings into the current tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchTarget {
    Pane(String),
    Tab(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Prev,
    Next,
}

/// A press that does nothing, with the words paneMorph shows for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoOp(pub String);

impl NoOp {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

pub const PANE_CLOSED: &str = "That pane closed";
pub const TAB_CLOSED: &str = "That tab closed";
pub const SPACE_CLOSED: &str = "That space closed";
pub const ALREADY_ALONE_TAB: &str = "Already alone in this tab";
pub const ALREADY_ALONE_SPACE: &str = "Already alone in this space";
pub const FIRST_TAB: &str = "Already the first tab";
pub const LAST_TAB: &str = "Already the last tab";
pub const ONLY_TAB: &str = "No other tab in this space";
pub const SAME_TAB: &str = "The pane is already in this tab";
pub const PANE_HERE: &str = "That pane is already here";
pub const TAB_HERE: &str = "That is this tab";
pub const NOTHING_TO_FETCH: &str = "Nothing to fetch: every pane is already here";
pub const NOTHING_TO_UNDO: &str = "Nothing to undo";

pub fn alone_in_tab(snapshot: &Snapshot, pane: &PaneInfo) -> bool {
    snapshot.pane_count_in_tab(&pane.tab_id) <= 1
}

pub fn alone_in_space(snapshot: &Snapshot, pane: &PaneInfo) -> bool {
    snapshot.pane_count_in_space(&pane.workspace_id) <= 1
}

/// Would sending `pane` into another space close its own space? Returns the
/// warning the Send row shows (edge case 1.7).
pub fn closes_space_warning(
    snapshot: &Snapshot,
    pane: &PaneInfo,
    target_workspace: &str,
) -> Option<String> {
    if target_workspace == pane.workspace_id || !alone_in_space(snapshot, pane) {
        return None;
    }
    let space = snapshot.workspace(&pane.workspace_id)?;
    Some(format!("closes {}", names::space_label(snapshot, space)))
}

/// Can Send move `pane` to `target`? (1.8, 1.9, 4.5, 4.6)
pub fn check_send(snapshot: &Snapshot, pane: &PaneInfo, target: &SendTarget) -> Result<(), NoOp> {
    match target {
        SendTarget::Tab(tab_id) => {
            if tab_id == &pane.tab_id {
                return Err(NoOp::new(SAME_TAB));
            }
            if snapshot.tab(tab_id).is_none() {
                return Err(NoOp::new(TAB_CLOSED));
            }
        }
        SendTarget::NewTabHere => {
            if alone_in_tab(snapshot, pane) {
                return Err(NoOp::new(ALREADY_ALONE_TAB));
            }
        }
        SendTarget::NewTabIn(workspace_id) => {
            if snapshot.workspace(workspace_id).is_none() {
                return Err(NoOp::new(SPACE_CLOSED));
            }
            if workspace_id == &pane.workspace_id && alone_in_tab(snapshot, pane) {
                return Err(NoOp::new(ALREADY_ALONE_TAB));
            }
        }
        SendTarget::NewSpace => {
            if alone_in_space(snapshot, pane) {
                return Err(NoOp::new(ALREADY_ALONE_SPACE));
            }
        }
    }
    Ok(())
}

/// The tab one step left or right in tab-bar order, without wrapping
/// (edge cases 4.1–4.4).
pub fn neighbour_tab(snapshot: &Snapshot, pane: &PaneInfo, step: Step) -> Result<String, NoOp> {
    let tabs = snapshot.tabs_in(&pane.workspace_id);
    if tabs.len() <= 1 {
        return Err(NoOp::new(ONLY_TAB));
    }
    let index = tabs
        .iter()
        .position(|t| t.tab_id == pane.tab_id)
        .ok_or_else(|| NoOp::new(TAB_CLOSED))?;
    let target = match step {
        Step::Prev if index == 0 => return Err(NoOp::new(FIRST_TAB)),
        Step::Prev => index - 1,
        Step::Next if index + 1 == tabs.len() => return Err(NoOp::new(LAST_TAB)),
        Step::Next => index + 1,
    };
    Ok(tabs[target].tab_id.clone())
}

/// Can Fetch bring `target` beside `dest`? (2.1, 2.15)
pub fn check_fetch(snapshot: &Snapshot, dest: &PaneInfo, target: &FetchTarget) -> Result<(), NoOp> {
    match target {
        FetchTarget::Pane(pane_id) => match snapshot.pane(pane_id) {
            None => Err(NoOp::new(PANE_CLOSED)),
            Some(pane) if pane.tab_id == dest.tab_id => Err(NoOp::new(PANE_HERE)),
            Some(_) => Ok(()),
        },
        FetchTarget::Tab(tab_id) => {
            if tab_id == &dest.tab_id {
                Err(NoOp::new(TAB_HERE))
            } else if snapshot.tab(tab_id).is_none() {
                Err(NoOp::new(TAB_CLOSED))
            } else {
                Ok(())
            }
        }
    }
}

/// Is there anything Fetch could bring here? (6.16)
pub fn anything_to_fetch(snapshot: &Snapshot, dest: &PaneInfo) -> bool {
    snapshot.panes.iter().any(|p| p.tab_id != dest.tab_id)
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

    fn pane<'a>(sim: &Sim, snapshot: &'a Snapshot, handle: &str) -> &'a PaneInfo {
        snapshot.pane(&sim.pane_id(handle)).unwrap()
    }

    /// Edge case 1.8: the current tab cannot be a Send destination.
    #[test]
    fn edge_1_8_send_into_own_tab_is_refused() {
        let (sim, snap) = setup();
        let p = pane(&sim, &snap, "portfolio");
        assert_eq!(
            check_send(&snap, p, &SendTarget::Tab(p.tab_id.clone())),
            Err(NoOp::new(SAME_TAB))
        );
    }

    /// Edge cases 1.9 and 4.5: a pane alone in its tab has no "new tab here".
    #[test]
    fn edge_1_9_and_4_5_alone_in_tab() {
        let (sim, snap) = setup();
        assert_eq!(
            check_send(&snap, pane(&sim, &snap, "zsh"), &SendTarget::NewTabHere),
            Err(NoOp::new(ALREADY_ALONE_TAB))
        );
        assert!(check_send(
            &snap,
            pane(&sim, &snap, "portfolio"),
            &SendTarget::NewTabHere
        )
        .is_ok());
    }

    /// Edge case 4.6: a pane alone in its space has no "new space".
    #[test]
    fn edge_4_6_alone_in_space() {
        let (sim, snap) = setup();
        assert_eq!(
            check_send(&snap, pane(&sim, &snap, "incident"), &SendTarget::NewSpace),
            Err(NoOp::new(ALREADY_ALONE_SPACE))
        );
    }

    /// Edge case 4.7: alone in its tab but not its space → New space works.
    #[test]
    fn edge_4_7_alone_in_tab_can_go_to_new_space() {
        let (sim, snap) = setup();
        assert!(check_send(&snap, pane(&sim, &snap, "zsh"), &SendTarget::NewSpace).is_ok());
    }

    /// Edge cases 4.1–4.3: tab-bar order, no wrap, no creation.
    #[test]
    fn edge_4_1_to_4_3_neighbours_follow_bar_order_without_wrap() {
        let (sim, snap) = setup();
        let first = pane(&sim, &snap, "portfolio");
        let last = pane(&sim, &snap, "scraper");
        assert_eq!(
            neighbour_tab(&snap, first, Step::Prev),
            Err(NoOp::new(FIRST_TAB))
        );
        assert_eq!(
            neighbour_tab(&snap, last, Step::Next),
            Err(NoOp::new(LAST_TAB))
        );
        assert_eq!(
            neighbour_tab(&snap, first, Step::Next).unwrap(),
            snap.tabs[1].tab_id
        );
        assert_eq!(
            neighbour_tab(&snap, last, Step::Prev).unwrap(),
            snap.tabs[1].tab_id
        );
    }

    /// Edge case 4.3: order comes from the snapshot, not tab numbers.
    #[test]
    fn edge_4_3_order_ignores_tab_numbers() {
        let (sim, _) = setup();
        // Move "Load tests" to the front; its number stays 3.
        api::tab_move(&sim, &sim.tab_id_of("scraper"), 0).unwrap();
        let snap = api::snapshot(&sim).unwrap();
        let p = snap.pane(&sim.pane_id("scraper")).unwrap();
        assert_eq!(
            neighbour_tab(&snap, p, Step::Prev),
            Err(NoOp::new(FIRST_TAB))
        );
        assert_eq!(
            neighbour_tab(&snap, p, Step::Next).unwrap(),
            sim.tab_id_of("portfolio")
        );
    }

    /// Edge case 4.4: one tab in the space.
    #[test]
    fn edge_4_4_only_one_tab() {
        let (sim, snap) = setup();
        let p = pane(&sim, &snap, "incident");
        assert_eq!(
            neighbour_tab(&snap, p, Step::Next),
            Err(NoOp::new(ONLY_TAB))
        );
        assert_eq!(
            neighbour_tab(&snap, p, Step::Prev),
            Err(NoOp::new(ONLY_TAB))
        );
    }

    /// Edge case 1.7: the Send row warns when the space would close.
    #[test]
    fn edge_1_7_warns_before_closing_a_space() {
        let (sim, snap) = setup();
        let p = pane(&sim, &snap, "incident");
        assert_eq!(
            closes_space_warning(&snap, p, "w1").as_deref(),
            Some("closes Payments_Service_Rewrite")
        );
        assert_eq!(
            closes_space_warning(&snap, pane(&sim, &snap, "zsh"), "w2"),
            None
        );
    }

    /// Edge cases 2.1 and 2.15: nothing from the current tab is fetchable.
    #[test]
    fn edge_2_1_and_2_15_fetch_refuses_current_tab() {
        let (sim, snap) = setup();
        let you = pane(&sim, &snap, "portfolio");
        let neighbour = pane(&sim, &snap, "headline");
        assert_eq!(
            check_fetch(&snap, you, &FetchTarget::Pane(neighbour.pane_id.clone())),
            Err(NoOp::new(PANE_HERE))
        );
        assert_eq!(
            check_fetch(&snap, you, &FetchTarget::Tab(you.tab_id.clone())),
            Err(NoOp::new(TAB_HERE))
        );
        let scraper = pane(&sim, &snap, "scraper");
        assert!(check_fetch(&snap, you, &FetchTarget::Pane(scraper.pane_id.clone())).is_ok());
    }

    /// Edge case 6.16: nothing to fetch when every pane is here.
    #[test]
    fn edge_6_16_nothing_to_fetch() {
        let sim = Sim::from_json(
            r#"{"spaces":[{"label":"solo","tabs":[{"layout":{"split":"right",
            "first":{"pane":{"name":"a","active":true}},"second":{"pane":{"name":"b"}}}}]}]}"#,
        )
        .unwrap();
        let snap = api::snapshot(&sim).unwrap();
        assert!(!anything_to_fetch(
            &snap,
            snap.pane(&sim.pane_id("a")).unwrap()
        ));
    }
}
