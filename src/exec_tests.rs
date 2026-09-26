//! Executor behaviour against the simulated herdr, one test per catalogue
//! row (numbers in the names refer to luxe/ive/edge-cases.md).

use std::path::PathBuf;

use crate::api;
use crate::exec::{Exec, Outcome};
use crate::journal::Journal;
use crate::model::SplitDir;
use crate::plan::{self, FetchTarget, SendTarget, Step};
use crate::sim::{Fault, Sim};
use crate::topology::Tree;

/// Two spaces; "work" has a three-pane tab, a one-pane tab and a two-pane
/// tab; "ops" has one tab with one pane.
const RICH: &str = r#"{"spaces":[
  {"label":"work","tabs":[
    {"label":"code","layout":{"split":"right","ratio":0.6,
      "first":{"pane":{"name":"editor","agent":"claude","cwd":"/r/app","active":true}},
      "second":{"split":"down","ratio":0.3,
        "first":{"pane":{"name":"logs","cwd":"/r/app","command":["tail","-f","x.log"]}},
        "second":{"pane":{"name":"tests","cwd":"/r/app","command":["cargo","test"]}}}}},
    {"layout":{"pane":{"name":"solo","cwd":"/r/solo"}}},
    {"label":"pair","layout":{"split":"down","ratio":0.7,
      "first":{"pane":{"name":"top","cwd":"/r/pair","focused":true}},
      "second":{"pane":{"name":"bottom","cwd":"/r/pair"}}}}
  ]},
  {"label":"ops","tabs":[
    {"label":"deploy","layout":{"pane":{"name":"deploy","agent":"codex","cwd":"/r/ops"}}}
  ]}
]}"#;

fn rich() -> Sim {
    Sim::from_json(RICH).unwrap()
}

fn done(outcome: Outcome) -> crate::exec::Done {
    match outcome {
        Outcome::Done(done) => done,
        other => panic!("expected a move, got {other:?}"),
    }
}

fn leaf(id: &str) -> Tree {
    Tree::leaf(id)
}
fn split(dir: SplitDir, ratio: f64, a: Tree, b: Tree) -> Tree {
    Tree::split(dir, ratio, a, b)
}

fn journal(name: &str) -> Journal {
    let dir: PathBuf = std::env::temp_dir().join(format!("pm-exec-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Journal::load(&dir, "sim")
}

fn send(sim: &Sim, handle: &str, target: SendTarget) -> crate::exec::Done {
    done(
        Exec::new(sim)
            .send(&sim.pane_id(handle), &target, SplitDir::Right, "send")
            .unwrap(),
    )
}

// ---------------------------------------------------------------- Sending

/// 1.1: beside the target tab's focused pane, right, ratio 0.5, focus follows.
#[test]
fn edge_1_1_send_to_existing_tab() {
    let sim = rich();
    let pair = sim.tab_id_of("top");
    let terminal = sim.terminal("logs");
    send(&sim, "logs", SendTarget::Tab(pair));
    assert_eq!(
        sim.tree_of("logs"),
        split(
            SplitDir::Down,
            0.7,
            split(SplitDir::Right, 0.5, leaf("top"), leaf("logs")),
            leaf("bottom")
        )
    );
    assert_eq!(
        sim.terminal("logs"),
        terminal,
        "same terminal keeps running"
    );
    assert_eq!(sim.focused().as_deref(), Some("logs"));
}

/// 1.14: ⇥ below uses split down.
#[test]
fn edge_1_14_send_below() {
    let sim = rich();
    let pair = sim.tab_id_of("top");
    Exec::new(&sim)
        .send(
            &sim.pane_id("logs"),
            &SendTarget::Tab(pair),
            SplitDir::Down,
            "send",
        )
        .unwrap();
    assert_eq!(
        sim.tree_of("logs"),
        split(
            SplitDir::Down,
            0.7,
            split(SplitDir::Down, 0.5, leaf("top"), leaf("logs")),
            leaf("bottom")
        )
    );
}

/// 1.15 and open questions 1–2: focus goes with the pane, and paneMorph
/// sends an explicit pane.focus so a herdr 0.9.0 client follows too.
#[test]
fn edge_1_15_focus_follows_with_explicit_focus_call() {
    let sim = rich();
    send(&sim, "logs", SendTarget::Tab(sim.tab_id_of("deploy")));
    let moves = sim.calls("pane.move");
    assert_eq!(moves.last().unwrap()["focus"], true);
    let focus = sim.calls("pane.focus");
    assert_eq!(focus.last().unwrap()["pane_id"], sim.pane_id("logs"));
    assert_eq!(sim.focused().as_deref(), Some("logs"));
}

/// 1.2 and 3.6: a new tab named after the agent, right after this tab.
#[test]
fn edge_1_2_new_tab_here_is_placed_next_to_current_tab() {
    let sim = rich();
    send(&sim, "editor", SendTarget::NewTabHere);
    // The untitled tab renumbers to "3" (edge case 3.12).
    assert_eq!(sim.tab_labels("work"), ["code", "claude", "3", "pair"]);
    assert_eq!(sim.where_is("editor").unwrap().1, "claude");
    assert_eq!(sim.focused().as_deref(), Some("editor"));
}

/// 3.1: a pane without an agent is named after its program.
#[test]
fn edge_3_1_new_tab_named_after_program() {
    let sim = rich();
    send(&sim, "tests", SendTarget::NewTabHere);
    assert_eq!(sim.where_is("tests").unwrap().1, "cargo");
}

/// 3.2: an existing name gets " 2".
#[test]
fn edge_3_2_duplicate_tab_name_gets_suffix() {
    let sim = Sim::from_json(r#"{"spaces":[{"label":"s","tabs":[{"label":"codex","layout":{"split":"right",
        "first":{"pane":{"name":"a","agent":"codex","active":true}},"second":{"pane":{"name":"b"}}}}]}]}"#).unwrap();
    send(&sim, "a", SendTarget::NewTabHere);
    assert_eq!(sim.tab_labels("s"), ["codex", "codex 2"]);
}

/// 3.7: the follow-up tab.move fails; the pane stays moved, with a warning.
#[test]
fn edge_3_7_tab_move_failure_keeps_the_move_and_warns() {
    let sim = rich();
    sim.inject(Fault::FailTabMove);
    let done = send(&sim, "editor", SendTarget::NewTabHere);
    assert_eq!(
        done.warnings,
        ["Moved, but couldn't place the new tab next to this one."]
    );
    // The untitled tab shows its position, "2" (edge case 3.12).
    assert_eq!(sim.tab_labels("work"), ["code", "2", "pair", "claude"]);
}

/// 1.3 and 1.18: another space's tab; new pane id, same terminal.
#[test]
fn edge_1_3_and_1_18_send_to_other_space() {
    let sim = rich();
    let before = sim.pane_id("logs");
    let terminal = sim.terminal("logs");
    let done = send(&sim, "logs", SendTarget::Tab(sim.tab_id_of("deploy")));
    assert_ne!(sim.pane_id("logs"), before);
    assert_eq!(done.entry.unwrap().terminals, [terminal]);
    assert_eq!(
        sim.where_is("logs").unwrap(),
        ("ops".into(), "deploy".into())
    );
}

/// 1.4 and 3.6: "New tab in ‹space›" goes to the end of that space.
#[test]
fn edge_1_4_new_tab_in_other_space_at_end() {
    let sim = rich();
    send(&sim, "tests", SendTarget::NewTabIn("w2".into()));
    assert_eq!(sim.tab_labels("ops"), ["deploy", "cargo"]);
    assert_eq!(sim.focused().as_deref(), Some("tests"));
}

/// 1.5, 3.3, 3.8: new space named after the folder, at the bottom.
#[test]
fn edge_1_5_3_3_3_8_new_space_named_after_folder_at_bottom() {
    let sim = rich();
    send(&sim, "editor", SendTarget::NewSpace);
    assert_eq!(sim.space_labels(), ["work", "ops", "app"]);
    assert_eq!(
        sim.where_is("editor").unwrap(),
        ("app".into(), "claude".into())
    );
}

/// 3.4: a used space name gets " 2".
#[test]
fn edge_3_4_duplicate_space_name_gets_suffix() {
    let sim = rich();
    send(&sim, "editor", SendTarget::NewSpace);
    send(&sim, "logs", SendTarget::NewSpace);
    assert_eq!(sim.space_labels(), ["work", "ops", "app", "app 2"]);
}

/// 1.6: the last pane of a tab; herdr closes the tab and the journal knows.
#[test]
fn edge_1_6_last_pane_closes_tab() {
    let sim = rich();
    let done = send(&sim, "solo", SendTarget::Tab(sim.tab_id_of("top")));
    assert_eq!(sim.tab_labels("work"), ["code", "pair"]);
    assert!(done.entry.unwrap().source_tab_closed);
}

/// 1.7: the last pane of a space sent elsewhere closes the space.
#[test]
fn edge_1_7_last_pane_closes_space() {
    let sim = rich();
    let done = send(&sim, "deploy", SendTarget::Tab(sim.tab_id_of("top")));
    assert_eq!(sim.space_labels(), ["work"]);
    assert!(done.entry.unwrap().source_space_closed);
}

/// 1.8, 1.9, 4.5, 4.6: refused targets move nothing.
#[test]
fn edge_1_9_4_5_4_6_noops_move_nothing() {
    let sim = rich();
    let exec = Exec::new(&sim);
    let solo = sim.pane_id("solo");
    assert_eq!(
        exec.send(
            &solo,
            &SendTarget::NewTabHere,
            SplitDir::Right,
            "move-tab-new"
        )
        .unwrap(),
        Outcome::NoOp(plan::NoOp::new(plan::ALREADY_ALONE_TAB))
    );
    let deploy = sim.pane_id("deploy");
    assert_eq!(
        exec.send(
            &deploy,
            &SendTarget::NewSpace,
            SplitDir::Right,
            "move-space-new"
        )
        .unwrap(),
        Outcome::NoOp(plan::NoOp::new(plan::ALREADY_ALONE_SPACE))
    );
    let tab = sim.tab_id_of("editor");
    assert!(matches!(
        exec.send(
            &sim.pane_id("editor"),
            &SendTarget::Tab(tab),
            SplitDir::Right,
            "send"
        )
        .unwrap(),
        Outcome::NoOp(_)
    ));
    assert!(sim.calls("pane.move").is_empty());
}

/// 1.10: a zoomed target tab is unzoomed first and stays unzoomed.
#[test]
fn edge_1_10_zoomed_target_is_unzoomed() {
    let sim = rich();
    api::pane_zoom(&sim, &sim.pane_id("top"), true).unwrap();
    api::pane_focus(&sim, &sim.pane_id("editor")).unwrap();
    send(&sim, "logs", SendTarget::Tab(sim.tab_id_of("top")));
    assert!(!sim.is_zoomed("top"));
    assert_eq!(sim.where_is("logs").unwrap().1, "pair");
}

/// 1.11 and 5.16: sending the zoomed pane unzooms its tab; undo zooms it again.
#[test]
fn edge_1_11_and_5_16_zoomed_source_round_trip() {
    let sim = rich();
    api::pane_zoom(&sim, &sim.pane_id("editor"), true).unwrap();
    let done = send(&sim, "editor", SendTarget::Tab(sim.tab_id_of("top")));
    let entry = done.entry.unwrap();
    assert_eq!(
        entry.rezoom_terminal.as_deref(),
        Some(sim.terminal("editor").as_str())
    );
    assert!(!sim.is_zoomed("logs"));
    let mut journal = journal("zoom");
    journal.push(*entry);
    let exec = Exec::new(&sim);
    done_undo(exec.undo(&mut journal, Some(&sim.pane_id("editor"))));
    assert_eq!(sim.where_is("editor").unwrap().1, "code");
    assert!(sim.is_zoomed("editor"));
}

fn done_undo(result: crate::exec::ExecResult) -> crate::exec::Done {
    done(result.unwrap())
}

/// 4.17 / 1.19: a quick key on a zoomed tab (an overlay's tab) unzooms first.
#[test]
fn edge_4_17_quick_key_on_zoomed_tab() {
    let sim = rich();
    api::pane_zoom(&sim, &sim.pane_id("logs"), true).unwrap();
    let snap = api::snapshot(&sim).unwrap();
    let logs = snap.pane(&sim.pane_id("logs")).unwrap();
    let next = plan::neighbour_tab(&snap, logs, Step::Next).unwrap();
    send(&sim, "logs", SendTarget::Tab(next));
    assert_eq!(sim.where_is("logs").unwrap().1, "2");
}

/// 4.8: two ⌃⌥→ presses act on the state the first left: two tabs over.
#[test]
fn edge_4_8_repeated_next_moves_two_tabs() {
    let sim = rich();
    let exec = Exec::new(&sim);
    for _ in 0..2 {
        let snap = api::snapshot(&sim).unwrap();
        let pane = snap.pane(&sim.pane_id("logs")).unwrap();
        let next = plan::neighbour_tab(&snap, pane, Step::Next).unwrap();
        exec.send(
            &pane.pane_id,
            &SendTarget::Tab(next),
            SplitDir::Right,
            "move-tab-next",
        )
        .unwrap();
    }
    assert_eq!(sim.where_is("logs").unwrap().1, "pair");
}

/// 6.6: a lost reply is checked against the snapshot before reporting.
#[test]
fn edge_6_6_lost_reply_is_verified() {
    let sim = rich();
    sim.inject(Fault::LoseMoveReply(0));
    let done = send(&sim, "logs", SendTarget::Tab(sim.tab_id_of("top")));
    let entry = done.entry.unwrap();
    assert_eq!(entry.dest_tab_id, sim.tab_id_of("top"));
    assert_eq!(sim.where_is("logs").unwrap().1, "pair");
}

/// 7.4: herdr's unchanged move becomes plain words.
#[test]
fn edge_7_4_unchanged_move_reason_in_plain_words() {
    let sim = rich();
    // A zoomed source that paneMorph does not know about: zoom after the
    // snapshot by sending a raw move that herdr refuses.
    api::pane_zoom(&sim, &sim.pane_id("top"), true).unwrap();
    let result = api::pane_move(
        &sim,
        &sim.pane_id("logs"),
        &api::MoveDest::Tab {
            tab_id: sim.tab_id_of("top"),
            target_pane_id: None,
            split: SplitDir::Right,
            ratio: 0.5,
        },
        false,
    )
    .unwrap();
    assert_eq!(result.reason.as_deref(), Some("zoomed_tab"));
}

// ---------------------------------------------------------------- Fetching

fn fetch(sim: &Sim, you: &str, target: FetchTarget, dir: SplitDir) -> crate::exec::Done {
    done(
        Exec::new(sim)
            .fetch(&sim.pane_id(you), &target, dir, "fetch")
            .unwrap(),
    )
}

/// 2.2: a pane from another tab lands right of you; you stay put.
#[test]
fn edge_2_2_fetch_pane_from_this_space() {
    let sim = rich();
    fetch(
        &sim,
        "editor",
        FetchTarget::Pane(sim.pane_id("top")),
        SplitDir::Right,
    );
    assert_eq!(sim.where_is("top").unwrap().1, "code");
    assert_eq!(sim.focused().as_deref(), Some("editor"));
    let tree = sim.tree_of("editor");
    assert_eq!(
        tree,
        split(
            SplitDir::Right,
            0.6,
            split(SplitDir::Right, 0.5, leaf("editor"), leaf("top")),
            split(SplitDir::Down, 0.3, leaf("logs"), leaf("tests"))
        )
    );
    assert_eq!(sim.calls("pane.move").last().unwrap()["focus"], false);
}

/// 2.3 and 2.7: a pane from another space; its space closes.
#[test]
fn edge_2_3_and_2_7_fetch_from_other_space_closes_it() {
    let sim = rich();
    let done = fetch(
        &sim,
        "editor",
        FetchTarget::Pane(sim.pane_id("deploy")),
        SplitDir::Down,
    );
    assert_eq!(sim.space_labels(), ["work"]);
    assert!(done.entry.unwrap().source_space_closed);
    assert_eq!(sim.focused().as_deref(), Some("editor"));
}

/// 2.4: a whole tab with its splits; the emptied tab closes; you stay.
#[test]
fn edge_2_4_fetch_whole_tab_rebuilds_splits() {
    let sim = rich();
    let done = fetch(
        &sim,
        "solo",
        FetchTarget::Tab(sim.tab_id_of("editor")),
        SplitDir::Right,
    );
    assert_eq!(
        sim.tree_of("solo"),
        split(
            SplitDir::Right,
            0.5,
            leaf("solo"),
            split(
                SplitDir::Right,
                0.6,
                leaf("editor"),
                split(SplitDir::Down, 0.3, leaf("logs"), leaf("tests"))
            )
        )
    );
    assert_eq!(sim.tab_labels("work"), ["1", "pair"]);
    assert_eq!(sim.focused().as_deref(), Some("solo"));
    assert_eq!(done.entry.unwrap().terminals.len(), 3);
}

/// 2.6: a whole tab from another space; ids change on every step.
#[test]
fn edge_2_6_fetch_whole_tab_across_spaces() {
    let sim = rich();
    fetch(
        &sim,
        "deploy",
        FetchTarget::Tab(sim.tab_id_of("top")),
        SplitDir::Right,
    );
    assert_eq!(
        sim.tree_of("deploy"),
        split(
            SplitDir::Right,
            0.5,
            leaf("deploy"),
            split(SplitDir::Down, 0.7, leaf("top"), leaf("bottom"))
        )
    );
    assert_eq!(sim.tab_labels("work"), ["code", "2"]);
}

/// 2.16: ⇥ below changes only the outer split.
#[test]
fn edge_2_16_whole_tab_below() {
    let sim = rich();
    fetch(
        &sim,
        "deploy",
        FetchTarget::Tab(sim.tab_id_of("top")),
        SplitDir::Down,
    );
    assert_eq!(
        sim.tree_of("deploy"),
        split(
            SplitDir::Down,
            0.5,
            leaf("deploy"),
            split(SplitDir::Down, 0.7, leaf("top"), leaf("bottom"))
        )
    );
}

/// 2.8: fetching into your zoomed tab unzooms it and leaves it unzoomed.
#[test]
fn edge_2_8_fetch_into_zoomed_tab() {
    let sim = rich();
    api::pane_zoom(&sim, &sim.pane_id("editor"), true).unwrap();
    fetch(
        &sim,
        "editor",
        FetchTarget::Pane(sim.pane_id("solo")),
        SplitDir::Right,
    );
    assert!(!sim.is_zoomed("editor"));
    assert_eq!(sim.where_is("solo").unwrap().1, "code");
}

/// 2.9: fetching from a zoomed tab elsewhere zooms its other pane again.
#[test]
fn edge_2_9_fetch_from_zoomed_tab_rezooms_it() {
    let sim = rich();
    api::pane_zoom(&sim, &sim.pane_id("logs"), true).unwrap();
    api::pane_focus(&sim, &sim.pane_id("top")).unwrap();
    fetch(
        &sim,
        "top",
        FetchTarget::Pane(sim.pane_id("tests")),
        SplitDir::Right,
    );
    assert!(sim.is_zoomed("logs"), "the code tab is zoomed again");
    assert!(!sim.is_zoomed("top"));
    assert_eq!(sim.where_is("tests").unwrap().1, "pair");
}

/// 2.10: the pane closed before ⏎.
#[test]
fn edge_2_10_fetch_closed_pane() {
    let sim = rich();
    let id = sim.pane_id("solo");
    sim.close_pane("solo");
    let result = Exec::new(&sim).fetch(
        &sim.pane_id("editor"),
        &FetchTarget::Pane(id),
        SplitDir::Right,
        "fetch",
    );
    assert_eq!(
        result.unwrap(),
        Outcome::NoOp(plan::NoOp::new(plan::PANE_CLOSED))
    );
}

/// 7.1: a whole-tab fetch failing midway puts the moved panes back exactly.
#[test]
fn edge_7_1_whole_tab_failure_rolls_back_exactly() {
    let sim = rich();
    let before = sim.tree_of("editor");
    sim.inject(Fault::FailMove(2));
    let error = Exec::new(&sim)
        .fetch(
            &sim.pane_id("deploy"),
            &FetchTarget::Tab(sim.tab_id_of("editor")),
            SplitDir::Right,
            "fetch",
        )
        .unwrap_err();
    assert!(
        error
            .message
            .starts_with("Couldn't fetch the whole tab; nothing changed."),
        "{}",
        error.message
    );
    assert!(error.entry.is_none());
    assert_eq!(sim.tree_of("editor"), before);
    assert_eq!(sim.tree_of("deploy"), leaf("deploy"));
}

/// 7.2: the rollback fails too; panes stay, the message lists them, and
/// the journal keeps an entry so ⌃⌥Z can retry.
#[test]
fn edge_7_2_rollback_failure_keeps_journal_entry() {
    let sim = rich();
    sim.inject(Fault::FailMove(2));
    sim.inject(Fault::FailMove(3));
    sim.inject(Fault::FailMove(4));
    let error = Exec::new(&sim)
        .fetch(
            &sim.pane_id("deploy"),
            &FetchTarget::Tab(sim.tab_id_of("editor")),
            SplitDir::Right,
            "fetch",
        )
        .unwrap_err();
    assert!(
        error.message.contains("Every process is still running"),
        "{}",
        error.message
    );
    let entry = error.entry.expect("journal entry for retry");
    assert!(entry.whole_tab);
    assert!(!entry.terminals.is_empty());
}

/// 7.11: paneMorph never calls destructive methods.
#[test]
fn edge_7_11_never_destroys_terminals() {
    let sim = rich();
    send(&sim, "editor", SendTarget::NewSpace);
    fetch(
        &sim,
        "solo",
        FetchTarget::Tab(sim.tab_id_of("top")),
        SplitDir::Right,
    );
    let mut j = journal("destroy");
    let exec = Exec::new(&sim);
    let _ = exec.undo(&mut j, None);
    for method in sim.all_calls() {
        assert!(
            !matches!(
                method.as_str(),
                "layout.apply" | "pane.close" | "tab.close" | "workspace.close"
            ),
            "{method}"
        );
    }
}

// ---------------------------------------------------------------- Undo

fn move_and_journal(sim: &Sim, j: &mut Journal, handle: &str, target: SendTarget) {
    let done = send(sim, handle, target);
    j.push(*done.entry.unwrap());
}

/// 5.5: back beside its former neighbour, exact geometry, second child.
#[test]
fn edge_5_5_undo_restores_exact_position() {
    let sim = rich();
    let before = sim.tree_of("tests");
    let mut j = journal("exact");
    move_and_journal(&sim, &mut j, "tests", SendTarget::Tab(sim.tab_id_of("top")));
    done_undo(Exec::new(&sim).undo(&mut j, Some(&sim.pane_id("tests"))));
    assert_eq!(sim.tree_of("tests"), before);
    assert!(j.entries.is_empty(), "5.3: undo is not journaled");
}

/// 5.5: a pane that was on the left/top is swapped into place.
#[test]
fn edge_5_5_undo_first_child_swaps_into_place() {
    let sim = rich();
    let before = sim.tree_of("top");
    let mut j = journal("swap");
    move_and_journal(
        &sim,
        &mut j,
        "top",
        SendTarget::Tab(sim.tab_id_of("editor")),
    );
    done_undo(Exec::new(&sim).undo(&mut j, Some(&sim.pane_id("top"))));
    assert_eq!(sim.tree_of("top"), before);
    assert!(!sim.calls("pane.swap").is_empty());
}

/// 5.4: undo after a Send takes you back with the pane.
#[test]
fn edge_5_4_undo_after_send_follows_the_pane() {
    let sim = rich();
    let mut j = journal("follow");
    move_and_journal(
        &sim,
        &mut j,
        "logs",
        SendTarget::Tab(sim.tab_id_of("deploy")),
    );
    done_undo(Exec::new(&sim).undo(&mut j, Some(&sim.pane_id("logs"))));
    assert_eq!(sim.focused().as_deref(), Some("logs"));
    assert_eq!(sim.where_is("logs").unwrap().1, "code");
}

/// 5.4: undo of a fetch sends the pane away and you stay.
#[test]
fn edge_5_4_undo_after_fetch_you_stay() {
    let sim = rich();
    let mut j = journal("stay");
    let done = fetch(
        &sim,
        "editor",
        FetchTarget::Pane(sim.pane_id("top")),
        SplitDir::Right,
    );
    j.push(*done.entry.unwrap());
    done_undo(Exec::new(&sim).undo(&mut j, Some(&sim.pane_id("editor"))));
    assert_eq!(sim.focused().as_deref(), Some("editor"));
    assert_eq!(sim.where_is("top").unwrap().1, "pair");
    assert_eq!(
        sim.tree_of("top"),
        split(SplitDir::Down, 0.7, leaf("top"), leaf("bottom"))
    );
}

/// 5.6: the former neighbour is gone: right of the tab's focused pane.
#[test]
fn edge_5_6_neighbour_gone() {
    let sim = rich();
    let mut j = journal("gone");
    move_and_journal(
        &sim,
        &mut j,
        "bottom",
        SendTarget::Tab(sim.tab_id_of("solo")),
    );
    // Nothing of the old tree remains once "top" leaves too.
    send(&sim, "editor", SendTarget::Tab(sim.tab_id_of("top")));
    fetch(
        &sim,
        "top",
        FetchTarget::Pane(sim.pane_id("logs")),
        SplitDir::Right,
    );
    sim.close_pane("top");
    // The pair tab now holds editor and logs only, none of bottom's neighbours.
    let result = Exec::new(&sim).undo(&mut j, None);
    done_undo(result);
    assert_eq!(sim.where_is("bottom").unwrap().1, "pair");
}

/// 5.7: the move had closed the source tab: recreated at its old position
/// with its old custom label; an untitled tab stays untitled.
#[test]
fn edge_5_7_undo_recreates_closed_tab_in_place() {
    let sim = rich();
    let mut j = journal("tab");
    move_and_journal(&sim, &mut j, "solo", SendTarget::Tab(sim.tab_id_of("top")));
    assert_eq!(sim.tab_labels("work"), ["code", "pair"]);
    done_undo(Exec::new(&sim).undo(&mut j, None));
    assert_eq!(sim.tab_labels("work"), ["code", "2", "pair"]);

    let sim = rich();
    let mut j = journal("tab2");
    // Empty the labelled "pair" tab into "code", then undo both.
    move_and_journal(
        &sim,
        &mut j,
        "bottom",
        SendTarget::Tab(sim.tab_id_of("editor")),
    );
    move_and_journal(
        &sim,
        &mut j,
        "top",
        SendTarget::Tab(sim.tab_id_of("editor")),
    );
    assert_eq!(sim.tab_labels("work"), ["code", "2"]);
    done_undo(Exec::new(&sim).undo(&mut j, None));
    done_undo(Exec::new(&sim).undo(&mut j, None));
    assert_eq!(sim.tab_labels("work"), ["code", "2", "pair"]);
    assert_eq!(
        sim.tree_of("top"),
        split(SplitDir::Down, 0.7, leaf("top"), leaf("bottom"))
    );
}

/// 5.8 and 5.9: the move closed the source space; undo recreates it at its
/// old sidebar position, and finds the pane by terminal id.
#[test]
fn edge_5_8_and_5_9_undo_recreates_closed_space() {
    let sim = Sim::from_json(
        r#"{"spaces":[
      {"label":"a","tabs":[{"layout":{"pane":{"name":"a1","active":true}}}]},
      {"label":"b","tabs":[{"label":"bt","layout":{"pane":{"name":"b1"}}}]},
      {"label":"c","tabs":[{"layout":{"pane":{"name":"c1"}}}]}]}"#,
    )
    .unwrap();
    let mut j = journal("space");
    move_and_journal(&sim, &mut j, "b1", SendTarget::Tab(sim.tab_id_of("a1")));
    assert_eq!(sim.space_labels(), ["a", "c"]);
    done_undo(Exec::new(&sim).undo(&mut j, None));
    assert_eq!(sim.space_labels(), ["a", "b", "c"]);
    assert_eq!(sim.where_is("b1").unwrap(), ("b".into(), "bt".into()));
}

/// 5.10: undo of a whole-tab fetch rebuilds the tab and its splits in place.
#[test]
fn edge_5_10_undo_whole_tab_fetch() {
    let sim = rich();
    let before = sim.tree_of("editor");
    let mut j = journal("whole");
    let done = fetch(
        &sim,
        "deploy",
        FetchTarget::Tab(sim.tab_id_of("editor")),
        SplitDir::Right,
    );
    j.push(*done.entry.unwrap());
    assert_eq!(sim.tab_labels("work"), ["1", "pair"]);
    done_undo(Exec::new(&sim).undo(&mut j, Some(&sim.pane_id("deploy"))));
    assert_eq!(sim.tab_labels("work"), ["code", "2", "pair"]);
    assert_eq!(sim.tree_of("editor"), before);
    assert_eq!(sim.tree_of("deploy"), leaf("deploy"));
}

/// 5.11: the pane closed; undo says so and drops the entry.
#[test]
fn edge_5_11_undo_closed_pane() {
    let sim = rich();
    let mut j = journal("closed");
    move_and_journal(&sim, &mut j, "logs", SendTarget::Tab(sim.tab_id_of("top")));
    move_and_journal(&sim, &mut j, "tests", SendTarget::Tab(sim.tab_id_of("top")));
    sim.close_pane("tests");
    let result = Exec::new(&sim).undo(&mut j, None).unwrap();
    assert_eq!(
        result,
        Outcome::Notice("Can't undo: that pane was closed".into())
    );
    assert_eq!(j.entries.len(), 1, "the next ⌃⌥Z tries the older one");
    done_undo(Exec::new(&sim).undo(&mut j, None));
    assert_eq!(sim.where_is("logs").unwrap().1, "code");
}

/// 5.12: the pane was moved by other means; undo refuses to guess.
#[test]
fn edge_5_12_undo_stale_after_manual_move() {
    let sim = rich();
    let mut j = journal("stale");
    move_and_journal(&sim, &mut j, "logs", SendTarget::Tab(sim.tab_id_of("top")));
    api::pane_move(
        &sim,
        &sim.pane_id("logs"),
        &api::MoveDest::NewTab {
            workspace_id: "w1".into(),
            label: None,
        },
        false,
    )
    .unwrap();
    let result = Exec::new(&sim).undo(&mut j, None).unwrap();
    assert_eq!(
        result,
        Outcome::Notice("Can't undo: that pane was moved since".into())
    );
    assert!(j.entries.is_empty());
}

/// 5.13: after a restart no terminal matches; the journal is cleared.
#[test]
fn edge_5_13_restart_clears_journal() {
    let sim = rich();
    let mut j = journal("restart");
    move_and_journal(&sim, &mut j, "logs", SendTarget::Tab(sim.tab_id_of("top")));
    let restarted = rich(); // same shape, new terminals
    for entry in &mut j.entries {
        entry.terminals = vec!["term-from-old-server".into()];
    }
    let result = Exec::new(&restarted).undo(&mut j, None).unwrap();
    assert_eq!(
        result,
        Outcome::NoOp(plan::NoOp::new(plan::NOTHING_TO_UNDO))
    );
    assert!(j.entries.is_empty());
}

/// 5.17: nothing to undo.
#[test]
fn edge_5_17_nothing_to_undo() {
    let sim = rich();
    let mut j = journal("empty");
    assert_eq!(
        Exec::new(&sim).undo(&mut j, None).unwrap(),
        Outcome::NoOp(plan::NoOp::new(plan::NOTHING_TO_UNDO))
    );
}

/// 5.3: each ⌃⌥Z undoes the next older move, walking a chain back.
#[test]
fn edge_5_3_undo_chain() {
    let sim = rich();
    let before = sim.tree_of("logs");
    let mut j = journal("chain");
    for _ in 0..2 {
        let snap = api::snapshot(&sim).unwrap();
        let pane = snap.pane(&sim.pane_id("logs")).unwrap();
        let next = plan::neighbour_tab(&snap, pane, Step::Next).unwrap();
        move_and_journal(&sim, &mut j, "logs", SendTarget::Tab(next));
    }
    assert_eq!(sim.where_is("logs").unwrap().1, "pair");
    done_undo(Exec::new(&sim).undo(&mut j, None));
    assert_eq!(sim.where_is("logs").unwrap().1, "2");
    done_undo(Exec::new(&sim).undo(&mut j, None));
    assert_eq!(sim.tree_of("logs"), before);
}

/// 5.18 via 7.2: an entry holding only the moved panes returns those.
#[test]
fn edge_5_18_partial_whole_tab_entry_returns_moved_panes() {
    let sim = rich();
    // Step 3 of the fetch fails, then the first rollback move fails too.
    sim.inject(Fault::FailMove(2));
    sim.inject(Fault::FailMove(3));
    let error = Exec::new(&sim)
        .fetch(
            &sim.pane_id("deploy"),
            &FetchTarget::Tab(sim.tab_id_of("editor")),
            SplitDir::Right,
            "fetch",
        )
        .unwrap_err();
    let mut j = journal("partial");
    j.push(*error.entry.unwrap());
    done_undo(Exec::new(&sim).undo(&mut j, None));
    assert_eq!(sim.where_is("editor").unwrap().1, "code");
    assert_eq!(sim.tree_of("deploy"), leaf("deploy"));
}

/// 7.3: herdr's own recovery after `pane_move_failed` puts the pane in a new
/// tab; paneMorph finds it by terminal id and still restores the source tab
/// exactly, and herdr closes the emptied recovery tab.
#[test]
fn edge_7_3_rollback_finds_a_pane_herdr_recovered() {
    let sim = rich();
    let before = sim.tree_of("editor");
    sim.inject(Fault::FailMoveRecovered(1));
    let error = Exec::new(&sim)
        .fetch(
            &sim.pane_id("deploy"),
            &FetchTarget::Tab(sim.tab_id_of("editor")),
            SplitDir::Right,
            "fetch",
        )
        .unwrap_err();
    assert!(
        error
            .message
            .starts_with("Couldn't fetch the whole tab; nothing changed."),
        "{}",
        error.message
    );
    assert_eq!(sim.tree_of("editor"), before);
    assert_eq!(sim.tab_labels("work"), ["code", "2", "pair"]);
    assert_eq!(sim.tree_of("deploy"), leaf("deploy"));
}

/// 5.1: a journal entry holds the terminal, its pane id after the move, the
/// source space and tab (id, label, position), the neighbour geometry (the
/// source split tree), whether the tab or space closed, and any zoom removed.
#[test]
fn edge_5_1_journal_entry_holds_what_undo_needs() {
    let sim = rich();
    api::pane_zoom(&sim, &sim.pane_id("logs"), true).unwrap();
    let done = send(&sim, "logs", SendTarget::Tab(sim.tab_id_of("deploy")));
    let entry = done.entry.unwrap();
    assert_eq!(entry.terminals, [sim.terminal("logs")]);
    assert_eq!(entry.pane_ids_after, [sim.pane_id("logs")]);
    assert_eq!(entry.source.workspace_label, "work");
    assert_eq!(entry.source.workspace_index, 0);
    assert_eq!(entry.source.tab_label.as_deref(), Some("code"));
    assert_eq!(entry.source.tab_index, 0);
    let tree = entry.source_tree.unwrap();
    assert_eq!(
        tree,
        split(
            SplitDir::Right,
            0.6,
            leaf(&sim.terminal("editor")),
            split(
                SplitDir::Down,
                0.3,
                leaf(&sim.terminal("logs")),
                leaf(&sim.terminal("tests"))
            )
        )
    );
    assert!(!entry.source_tab_closed && !entry.source_space_closed);
    assert_eq!(entry.rezoom_terminal, Some(sim.terminal("logs")));
    assert!(entry.followed);
}

/// 7.9: a log line holds the action, the terminal, the pane id before and
/// after, the source and destination tabs, the result and the duration.
#[test]
fn edge_7_9_log_line_contents() {
    let sim = rich();
    let before = sim.pane_id("logs");
    let from = sim.tab_id_of("logs");
    let done = send(&sim, "logs", SendTarget::Tab(sim.tab_id_of("deploy")));
    let log = done.log;
    assert!(log.starts_with("send terminal="), "{log}");
    assert!(log.contains(&sim.terminal("logs")));
    assert!(log.contains(&format!("pane={before}->{}", sim.pane_id("logs"))));
    assert!(log.contains(&format!("from={from} to={}", sim.tab_id_of("logs"))));
    assert!(log.contains("result=changed"));
    assert!(log.trim_end().ends_with("ms"));
}

/// 3.10: paneMorph never asks for a name; it passes the names it chose.
#[test]
fn edge_3_10_names_are_passed_never_prompted() {
    let sim = rich();
    send(&sim, "editor", SendTarget::NewTabHere);
    send(&sim, "logs", SendTarget::NewSpace);
    let moves = sim.calls("pane.move");
    assert_eq!(moves[0]["destination"]["label"], "claude");
    assert_eq!(moves[1]["destination"]["label"], "app");
    assert_eq!(moves[1]["destination"]["tab_label"], "tail");
}
