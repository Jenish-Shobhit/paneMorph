//! Split trees and the ordered pane moves that rebuild them.
//!
//! herdr's `pane.move` only ever inserts the moved pane as the *second*
//! child (right or below) of a new split around one target pane. Every plan
//! here is a sequence of such inserts, plus an optional `pane.swap` when a
//! pane must end up first (left or top).

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::model::{LayoutNode, SplitDir};

/// A split tree whose leaves are pane ids or terminal ids.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Tree {
    Leaf {
        id: String,
    },
    Split {
        dir: SplitDir,
        ratio: f64,
        first: Box<Tree>,
        second: Box<Tree>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum TopologyError {
    MissingPaneId,
    BadRatio,
}

impl std::fmt::Display for TopologyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingPaneId => write!(f, "a layout pane is missing its pane id"),
            Self::BadRatio => write!(f, "a layout split has an invalid ratio"),
        }
    }
}

impl Tree {
    pub fn leaf(id: impl Into<String>) -> Self {
        Self::Leaf { id: id.into() }
    }

    pub fn split(dir: SplitDir, ratio: f64, first: Tree, second: Tree) -> Self {
        Self::Split {
            dir,
            ratio,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    pub fn from_layout(node: &LayoutNode) -> Result<Self, TopologyError> {
        match node {
            LayoutNode::Pane { pane_id } => match pane_id {
                Some(id) if !id.is_empty() => Ok(Self::leaf(id.clone())),
                _ => Err(TopologyError::MissingPaneId),
            },
            LayoutNode::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                if !(*ratio > 0.0 && *ratio < 1.0) {
                    return Err(TopologyError::BadRatio);
                }
                Ok(Self::split(
                    *direction,
                    *ratio,
                    Self::from_layout(first)?,
                    Self::from_layout(second)?,
                ))
            }
        }
    }

    pub fn first_leaf(&self) -> &str {
        match self {
            Self::Leaf { id } => id,
            Self::Split { first, .. } => first.first_leaf(),
        }
    }

    pub fn leaves(&self) -> Vec<&str> {
        let mut out = Vec::new();
        self.collect_leaves(&mut out);
        out
    }

    fn collect_leaves<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Self::Leaf { id } => out.push(id),
            Self::Split { first, second, .. } => {
                first.collect_leaves(out);
                second.collect_leaves(out);
            }
        }
    }

    pub fn contains(&self, id: &str) -> bool {
        self.leaves().contains(&id)
    }

    /// Rename every leaf; leaves missing from `map` keep their id.
    pub fn map_leaves(&self, map: &HashMap<String, String>) -> Tree {
        match self {
            Self::Leaf { id } => Self::leaf(map.get(id).cloned().unwrap_or_else(|| id.clone())),
            Self::Split {
                dir,
                ratio,
                first,
                second,
            } => Self::split(*dir, *ratio, first.map_leaves(map), second.map_leaves(map)),
        }
    }

    /// The tree herdr keeps after every leaf outside `keep` is removed: a
    /// removed leaf's sibling takes its parent's place.
    pub fn prune(&self, keep: &HashSet<String>) -> Option<Tree> {
        match self {
            Self::Leaf { id } => keep.contains(id).then(|| self.clone()),
            Self::Split {
                dir,
                ratio,
                first,
                second,
            } => match (first.prune(keep), second.prune(keep)) {
                (Some(a), Some(b)) => Some(Self::split(*dir, *ratio, a, b)),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            },
        }
    }

    /// Find the split whose direct child is leaf `id`.
    fn parent_of(&self, id: &str) -> Option<(SplitDir, f64, bool, &Tree)> {
        match self {
            Self::Leaf { .. } => None,
            Self::Split {
                dir,
                ratio,
                first,
                second,
            } => {
                if matches!(first.as_ref(), Self::Leaf { id: leaf } if leaf == id) {
                    return Some((*dir, *ratio, true, second));
                }
                if matches!(second.as_ref(), Self::Leaf { id: leaf } if leaf == id) {
                    return Some((*dir, *ratio, false, first));
                }
                first.parent_of(id).or_else(|| second.parent_of(id))
            }
        }
    }
}

/// One live `pane.move` into an existing tab.
#[derive(Debug, Clone, PartialEq)]
pub struct MoveStep {
    pub pane_id: String,
    pub tab_id: String,
    pub target_pane_id: String,
    pub split: SplitDir,
    pub ratio: f64,
}

/// Plan the moves that recreate `source` beside `dest_pane` in `dest_tab`
/// (edge cases 2.4, 2.16).
///
/// The first source leaf lands beside the destination pane using the chosen
/// `outer` direction. Each later step expands one placeholder leaf using the
/// source node's own direction and ratio, so inner splits keep theirs.
pub fn plan_tab_merge(
    source: &Tree,
    dest_tab: &str,
    dest_pane: &str,
    outer: SplitDir,
    outer_ratio: f64,
) -> Vec<MoveStep> {
    let mut steps = vec![MoveStep {
        pane_id: source.first_leaf().to_string(),
        tab_id: dest_tab.to_string(),
        target_pane_id: dest_pane.to_string(),
        split: outer,
        ratio: outer_ratio,
    }];
    expansion_steps(source, dest_tab, &mut steps);
    steps
}

/// The moves that grow a tree from its first leaf (used to rebuild a tab
/// whose first pane is already in place, edge case 5.10).
pub fn expansion_steps(node: &Tree, tab: &str, out: &mut Vec<MoveStep>) {
    if let Tree::Split {
        dir,
        ratio,
        first,
        second,
    } = node
    {
        out.push(MoveStep {
            pane_id: second.first_leaf().to_string(),
            tab_id: tab.to_string(),
            target_pane_id: first.first_leaf().to_string(),
            split: *dir,
            ratio: *ratio,
        });
        expansion_steps(first, tab, out);
        expansion_steps(second, tab, out);
    }
}

/// Where to put one leaf back into a tab that currently holds `present`.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    /// Leaf to split around.
    pub anchor: String,
    pub split: SplitDir,
    pub ratio: f64,
    /// The leaf used to be first (left or top): swap after inserting.
    pub swap: bool,
    /// The anchor is the whole original sibling, so the result is exact.
    pub exact: bool,
}

/// Compute how to return leaf `id` to a tab whose current tree is `original`
/// pruned to `present` (edge cases 5.5, 7.1). Returns `None` when no recorded
/// neighbour is present (edge case 5.6 then applies).
pub fn reinsertion(original: &Tree, present: &HashSet<String>, id: &str) -> Option<Placement> {
    let mut keep = present.clone();
    keep.insert(id.to_string());
    let pruned = original.prune(&keep)?;
    let (split, ratio, is_first, sibling) = pruned.parent_of(id)?;
    Some(Placement {
        anchor: sibling.first_leaf().to_string(),
        split,
        ratio,
        swap: is_first,
        exact: matches!(sibling, Tree::Leaf { .. }),
    })
}

/// Order in which to return `missing` leaves so that every insertion is
/// exact when that is possible at all. A small backtracking search looks for
/// an all-exact order; if none exists (herdr cannot split around a group of
/// panes), a greedy order is used instead.
pub fn return_order(original: &Tree, present: &HashSet<String>, missing: &[String]) -> Vec<String> {
    let mut budget = 20_000usize;
    let mut order = Vec::new();
    let mut have = present.clone();
    let mut left = missing.to_vec();
    if exact_search(original, &mut have, &mut left, &mut order, &mut budget) {
        return order;
    }
    greedy_order(original, present, missing)
}

fn exact_search(
    original: &Tree,
    have: &mut HashSet<String>,
    left: &mut Vec<String>,
    order: &mut Vec<String>,
    budget: &mut usize,
) -> bool {
    if left.is_empty() {
        return true;
    }
    if *budget == 0 {
        return false;
    }
    *budget -= 1;
    for index in 0..left.len() {
        let id = left[index].clone();
        if !reinsertion(original, have, &id).is_some_and(|p| p.exact) {
            continue;
        }
        left.remove(index);
        have.insert(id.clone());
        order.push(id.clone());
        if exact_search(original, have, left, order, budget) {
            return true;
        }
        order.pop();
        have.remove(&id);
        left.insert(index, id);
    }
    false
}

fn greedy_order(original: &Tree, present: &HashSet<String>, missing: &[String]) -> Vec<String> {
    let mut present = present.clone();
    let mut left: Vec<String> = missing.to_vec();
    let mut order = Vec::new();
    while !left.is_empty() {
        let pick = left
            .iter()
            .position(|id| {
                reinsertion(original, &present, id).is_some_and(|placement| placement.exact)
            })
            .or_else(|| {
                left.iter()
                    .position(|id| reinsertion(original, &present, id).is_some())
            })
            .unwrap_or(0);
        let id = left.remove(pick);
        present.insert(id.clone());
        order.push(id);
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(id: &str) -> Tree {
        Tree::leaf(id)
    }
    fn split(dir: SplitDir, ratio: f64, a: Tree, b: Tree) -> Tree {
        Tree::split(dir, ratio, a, b)
    }
    fn set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    /// A tiny model of herdr's insert: split `target` with `moved` second.
    fn insert(tree: &Tree, target: &str, moved: &str, dir: SplitDir, ratio: f64) -> Tree {
        match tree {
            Tree::Leaf { id } if id == target => split(dir, ratio, leaf(id), leaf(moved)),
            Tree::Leaf { .. } => tree.clone(),
            Tree::Split {
                dir: d,
                ratio: r,
                first,
                second,
            } => split(
                *d,
                *r,
                insert(first, target, moved, dir, ratio),
                insert(second, target, moved, dir, ratio),
            ),
        }
    }

    fn swap(tree: &Tree, a: &str, b: &str) -> Tree {
        let map: HashMap<String, String> = [
            (a.to_string(), b.to_string()),
            (b.to_string(), a.to_string()),
        ]
        .into_iter()
        .collect();
        tree.map_leaves(&map)
    }

    #[test]
    fn single_pane_tab_becomes_one_right_move() {
        let steps = plan_tab_merge(&leaf("w1:p1"), "w1:t2", "w1:p9", SplitDir::Right, 0.5);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].target_pane_id, "w1:p9");
        assert_eq!(steps[0].split, SplitDir::Right);
    }

    #[test]
    fn nested_tree_preserves_source_directions_and_ratios() {
        let root = split(
            SplitDir::Down,
            0.6,
            split(SplitDir::Right, 0.4, leaf("a"), leaf("b")),
            leaf("c"),
        );
        let steps = plan_tab_merge(&root, "t9", "you", SplitDir::Right, 0.55);
        let ids: Vec<_> = steps.iter().map(|s| s.pane_id.as_str()).collect();
        assert_eq!(ids, ["a", "c", "b"]);
        let targets: Vec<_> = steps.iter().map(|s| s.target_pane_id.as_str()).collect();
        assert_eq!(targets, ["you", "a", "a"]);
        let ratios: Vec<_> = steps.iter().map(|s| s.ratio).collect();
        assert_eq!(ratios, [0.55, 0.6, 0.4]);
    }

    /// Edge case 2.16: ⇥ below only changes the outer split.
    #[test]
    fn edge_2_16_below_only_changes_outer_split() {
        let root = split(SplitDir::Right, 0.3, leaf("a"), leaf("b"));
        let steps = plan_tab_merge(&root, "t", "you", SplitDir::Down, 0.5);
        assert_eq!(steps[0].split, SplitDir::Down);
        assert_eq!(steps[1].split, SplitDir::Right);
        assert_eq!(steps[1].ratio, 0.3);
    }

    /// Replaying the plan against a herdr-like insert rebuilds the tree exactly.
    #[test]
    fn plan_replay_rebuilds_the_exact_tree() {
        let root = split(
            SplitDir::Right,
            0.35,
            split(
                SplitDir::Down,
                0.7,
                leaf("a"),
                split(SplitDir::Right, 0.5, leaf("b"), leaf("c")),
            ),
            split(SplitDir::Down, 0.25, leaf("d"), leaf("e")),
        );
        let mut tree = leaf("a");
        let mut steps = Vec::new();
        expansion_steps(&root, "t", &mut steps);
        for step in steps {
            tree = insert(
                &tree,
                &step.target_pane_id,
                &step.pane_id,
                step.split,
                step.ratio,
            );
        }
        assert_eq!(tree, root);
    }

    #[test]
    fn prune_collapses_like_herdr() {
        let root = split(
            SplitDir::Down,
            0.6,
            split(SplitDir::Right, 0.4, leaf("a"), leaf("b")),
            leaf("c"),
        );
        assert_eq!(
            root.prune(&set(&["a", "c"])),
            Some(split(SplitDir::Down, 0.6, leaf("a"), leaf("c")))
        );
        assert_eq!(root.prune(&set(&["b"])), Some(leaf("b")));
        assert_eq!(root.prune(&set(&[])), None);
    }

    /// Edge case 5.5: a pane that was second goes back beside its neighbour.
    #[test]
    fn edge_5_5_second_child_returns_without_swap() {
        let root = split(SplitDir::Right, 0.3, leaf("editor"), leaf("server"));
        let placement = reinsertion(&root, &set(&["editor"]), "server").unwrap();
        assert_eq!(placement.anchor, "editor");
        assert_eq!(placement.ratio, 0.3);
        assert!(!placement.swap && placement.exact);
        let rebuilt = insert(
            &leaf("editor"),
            "editor",
            "server",
            placement.split,
            placement.ratio,
        );
        assert_eq!(rebuilt, root);
    }

    /// Edge case 5.5: a pane that was first (left/top) is inserted then swapped.
    #[test]
    fn edge_5_5_first_child_returns_with_swap() {
        let root = split(SplitDir::Down, 0.7, leaf("logs"), leaf("shell"));
        let placement = reinsertion(&root, &set(&["shell"]), "logs").unwrap();
        assert!(placement.swap);
        let inserted = insert(
            &leaf("shell"),
            "shell",
            "logs",
            placement.split,
            placement.ratio,
        );
        assert_eq!(swap(&inserted, "logs", "shell"), root);
    }

    /// Edge case 5.5: a neighbour group means "beside the group's first pane".
    #[test]
    fn edge_5_5_group_neighbour_uses_first_pane_and_is_not_exact() {
        let root = split(
            SplitDir::Right,
            0.5,
            leaf("x"),
            split(SplitDir::Down, 0.5, leaf("b"), leaf("c")),
        );
        let placement = reinsertion(&root, &set(&["b", "c"]), "x").unwrap();
        assert_eq!(placement.anchor, "b");
        assert!(placement.swap);
        assert!(!placement.exact);
    }

    /// Edge case 5.6: no recorded neighbour left.
    #[test]
    fn edge_5_6_no_neighbour_left() {
        let root = split(SplitDir::Right, 0.5, leaf("a"), leaf("b"));
        assert_eq!(reinsertion(&root, &set(&[]), "b"), None);
    }

    /// Edge case 7.1: greedy return order restores a partial whole-tab move
    /// exactly for the usual shapes.
    #[test]
    fn edge_7_1_return_order_restores_three_pane_tab_exactly() {
        let root = split(
            SplitDir::Down,
            0.6,
            split(SplitDir::Right, 0.4, leaf("a"), leaf("b")),
            leaf("c"),
        );
        // Plan order a, c, b; the move of b failed, so a and c had moved.
        let present = set(&["b"]);
        let order = return_order(&root, &present, &["a".into(), "c".into()]);
        let mut tree = leaf("b");
        let mut have = present.clone();
        for id in order {
            let p = reinsertion(&root, &have, &id).unwrap();
            assert!(p.exact, "{id} should return exactly");
            tree = insert(&tree, &p.anchor, &id, p.split, p.ratio);
            if p.swap {
                tree = swap(&tree, &id, &p.anchor);
            }
            have.insert(id);
        }
        assert_eq!(tree, root);
    }

    #[test]
    fn from_layout_rejects_missing_ids() {
        let node = LayoutNode::Pane { pane_id: None };
        assert_eq!(Tree::from_layout(&node), Err(TopologyError::MissingPaneId));
    }
}
