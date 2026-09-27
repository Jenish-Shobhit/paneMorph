# paneMorph 0.2.0 verification

This page covers how paneMorph 0.2.0 was tested, what the tests found, and the
answers to the twelve open questions in the redesign's edge-case catalogue
(`edge-cases.md`, 116 rows). Row numbers such as 1.16 refer to that catalogue.

Tested on macOS (arm64) against herdr 0.9.0 (Homebrew) and herdr 0.9.1 (the
release binary), on 27 September 2026.

## How it was tested

Every live test ran on a **throwaway herdr server**, never the working
session. Each run used:

- a private `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, `XDG_DATA_HOME` and
  `XDG_CACHE_HOME` under a temporary directory, so it had its own
  `config.toml`, `plugins.json`, session, sockets and plugin state;
- a named session `pmtest-<time>` inside that directory;
- no inherited `HERDR_*` variables;
- this checkout registered only in the private `plugins.json`. No
  `herdr plugin link` was run and the global registry was never written;
- a guard that refuses any socket path outside the private directory.

A real herdr client ran attached inside a pseudo-terminal, and tests pressed
real key bytes through it. For example, ⌃⌥S arrives as `ESC 0x13`. Screens
were rendered from the client's output with a VT100 parser. Checksums of the
working session's `~/.config/herdr/plugins.json` and `config.toml` were
identical before and after every run.

| Suite | Where | Result |
| --- | --- | --- |
| Unit and rendering tests: planners, naming, journal, executor against a herdr simulator, row builders, key handling, ratatui `TestBackend` rendering | `cargo test` (`src/**`) | 139 passed |
| Pseudo-terminal tests of the real binary: rendering, filtering, drill-in, window switching, mouse, resize, ⎋ and ⌃C, unreachable herdr | `tests/pty_windows.rs` | 9 passed (1 ignored screen dump) |
| Live test in the repository: Send, Fetch, every quick key and undo through a real client | `tests/live_herdr.rs` (opt-in) | passed on 0.9.0 and on 0.9.1 |
| Live edge-case and open-question probes (scratch harness, not shipped) | throwaway sessions | 60/60 on 0.9.0; 51/51 on 0.9.1 (core set) |

`cargo clippy --all-targets` and `cargo fmt --check` are clean.

### Live results (herdr 0.9.0)

Every check below passed. Unless stated otherwise, the same checks also passed
on 0.9.1.

- **Send window.** Opens as a popup on ⌃⌥S, and nothing zooms (6.1). It shows
  the design's rows, and the border reads "paneMorph · send" (6.19).
- **Send to an existing tab.** The pane lands right of the focused pane at
  0.5 (1.1). Server focus and the attached client both follow it (1.15,
  1.16).
- **New tab here.** Named after the pane's folder or command, and placed next
  to the current tab (1.2, 3.1, 3.6).
- **Another space.** Drilling in and sending works; the pane gets a new id and
  keeps its terminal (1.3, 1.18). The Send row warns "closes beta" before a
  space would close (1.7).
- **⌃⌥N.** Opens a new space at the bottom, named after the folder (1.5,
  3.3). Pressed again, it does nothing because the pane is alone (4.6).
- **⌃⌥T.** Opens a new tab next to the current one; pressed again, it does
  nothing (4.5).
- **⌃⌥→ and ⌃⌥←.** Move the pane one tab; the tab it leaves closes if it was
  the last pane (1.6). Three fast ⌃⌥→ presses queue and stop at the last tab
  (4.8, 4.1).
- **Fetch.** Panes are listed by command (`sleep 8888`), never by id (2.12,
  2.13). A fetched pane lands beside you and you stay put (2.2). A whole tab
  arrives with its split directions and ratios (2.4).
- **Undo.**
  - Restores the exact split tree after a Send, including left or top panes
    via swap (5.5).
  - Follows the pane after a Send and stays put after a Fetch (5.4).
  - Works after a cross-space move (5.9) and walks back a chain of moves
    (5.3).
  - Recreates a closed tab with its label and position (5.7), and a closed
    space in its sidebar slot (5.8).
  - Rebuilds a whole-tab fetch in place (5.10) and zooms the pane again when
    paneMorph had unzoomed it (5.16).
- **Zoom.** A zoomed target tab is unzoomed and left unzoomed (1.10). A quick
  key on a zoomed tab unzooms it first (1.11, 4.17).
- **Other plugins' panes.** Another plugin's overlay pane, whose tab is
  zoomed, moves like any pane after the unzoom (1.19).
- **Popup lifecycle.** Moving the last pane of the popup's own tab through
  Send closes that tab. The move is still journaled and undoable (1.6, 6.2,
  6.20).
- **Window keys.** ⌃⌥F inside Send reopens as Fetch, and ⌃⌥S goes back
  (4.10). The window stays open and refreshes when a pane closes underneath
  it (6.15).
- **Errors and notices.**
  - With herdr's default `delivery = "off"`, a failed quick key opens the
    notice popup, which closes after 4.0 s (7.6).
  - With `delivery = "herdr"`, it shows a herdr toast instead (7.6).
  - With another popup open, the notice is only logged (7.7).
  - A second Send while one is open is refused by herdr and logged as
    "window already open" (6.3).
  - `extract-pane` still works and logs a deprecation line (4.15).
- **Safety.** Every original terminal was still running at the end, the
  layout matched the start, and no paneMorph action failed (7.10, 7.11).

## The twelve open questions

**1. On herdr 0.9.1, does `pane.move` with `focus: true` move the attached
window across tabs and spaces, including when the source tab closes?**
Yes. The client followed the moved pane within 10 ms for a move to another tab
in the same space and for a move to another space, with no extra call. When the
tab the client was viewing closed because its only pane moved, the client
followed the pane; 0.9.0 does the same in that case.

**2. On 0.9.0, do `tab.focus` or `workspace.focus` move the attached window?**
Yes. `tab.focus`, `workspace.focus` and `pane.focus` all move an attached
0.9.0 client. `pane.move` with `focus: true` alone does not: in 24 of 24
trials, typing still reached the pane the client had been viewing (herdr issue
#4153). paneMorph therefore keeps `min_herdr_version = "0.9.0"` and calls
`pane.focus` on the moved pane after every move that should take you along
(Send, the quick keys, and an undo you follow). Measured on 0.9.0 with that
call, the client had not followed at 0 ms, followed in 1 of 4 trials at 10 ms,
and followed in every trial at 25 ms and later. On 0.9.1 without the call, it
followed in every trial from 10 ms. A key pressed within about 25 ms of a move
finishing can therefore still act on the pane you left on 0.9.0. The call is
harmless on 0.9.1.

**3. With two clients viewing different tabs, whose focused pane does a
keybinding put in `HERDR_PLUGIN_CONTEXT_JSON` → `focused_pane_id`?**
The pressing client's. Client 1 viewed pane A and client 2 viewed pane B.
Pressing ⌃⌥S on client 1 opened Send for pane A, and the popup appeared only on
client 1.

**4. Does a popup honour kitty keyboard flags pushed by its own process, so
that ⇧⏎ arrives as `ESC[13;2u`?**
Not tested live, and not needed: the approved design drops ⇧⏎ (2.5). ⏎ on a
tab row fetches the whole tab. From the source, herdr encodes keys for a popup
through the same terminal runtime as a pane, and sends `ESC[13;2u` once the
child has pushed kitty flags, so it probably does.

**5. Does Ghostty on macOS pass all seven ⌃⌥ chords through to herdr, and
which bytes reach the popup?**
Partly answered.

- Ghostty 1.3.1's default key table (`ghostty +list-keybinds --default`) has no
  ctrl+alt binding. The nearest defaults are `alt+arrow_left/right`
  (`esc:b`/`esc:f`) and `super+ctrl+arrows` (resize split). The test machine's
  Ghostty config adds no ctrl+alt binding either.
- In a popup, herdr's legacy encoding (`src/input/encode.rs`) delivers these
  bytes: ⌃⌥S = `ESC 0x13`, ⌃⌥F = `ESC 0x06`, ⌃⌥T = `ESC 0x14`,
  ⌃⌥N = `ESC 0x0e`, ⌃⌥Z = `ESC 0x1a`, ⌃⌥← = `ESC[1;7D`, ⌃⌥→ = `ESC[1;7C`.
- The live tests typed exactly these bytes into a real herdr client, which
  triggered all seven bindings. Inside a window, ⌃⌥S and ⌃⌥F switched windows
  and the others were ignored.
- Not verified: pressing the chords on a physical keyboard in the running
  Ghostty, and macOS system shortcuts. Both would need the working session.

**6. Does the popup border draw the manifest title?**
Yes. The border reads `┌paneMorph · send──…┐`, and the same for Fetch.

**7. What does the window show if the popup's owner tab closes, or focus moves
away, before the popup exits?**
Nothing: herdr closes the popup and its process exits as soon as the owner tab
closes. Sending the last pane of the popup's own tab triggers exactly this.
The window therefore hands every move to a detached `panemorph apply` worker,
which finishes the move, focus, tab placement, journal and log, and reports any
late problem as a notice (6.2, 6.20).

**8. Do `terminal_id`s survive a live handoff?**
No. After `server.live_handoff` on a throwaway 0.9.0 server, pane ids, shell
pids and scrollback were unchanged, but every `terminal_id` was new. The undo
journal is keyed by terminal id, so after a handoff ⌃⌥Z finds no match, says
"Nothing to undo" and clears the journal. That is the same as after a restart
(5.13), and it never guesses (5.14).

**9. If `[keys] zoom = "ctrl+alt+z"` and a `[[keys.command]]` on ctrl+alt+z
both exist, which one wins, and does `herdr config check` flag the clash?**
Zoom wins, and it is flagged. `herdr config check` exits 1 with "ctrl+alt+z:
kept keys.zoom, disabled keys.command[6].key".
`server.reload_config` answers `partial` with the same diagnostic. Pressing
⌃⌥Z never ran undo.

**10. How long do `pane.process_info` calls take across 50 or more panes?**
52 panes took 26 to 30 ms in total: 0.5 to 0.6 ms each on average, at most 5 ms.
One `session.snapshot` took 3 to 4 ms. The Fetch window draws from the snapshot
first and fills in commands in the background, visible rows first. With 58
panes it took 225 to 259 ms from key press to drawn window, most of it herdr
starting the popup.

**11. Does a runtime spawn failure, such as a missing `python3`, appear in
`herdr plugin log list`?**
Yes. A missing program, relative or on PATH, logs `status: failed`, no exit
code, and `error: "No such file or directory (os error 2)"`. An unbuilt
paneMorph checkout logs exit code 127 with "paneMorph is not built. Run: cargo
build --release (in …)". `./bin/panemorph doctor` checks the socket and
versions (7.13).

**12. Is herdr 0.9.1 in Homebrew yet?**
Yes. `brew info herdr` shows stable 0.9.1 (bottled); the test machine had 0.9.0.
herdr v0.9.1 was released on 16 September 2026.

## Other findings

- **Toasts.** On 0.9.0 and 0.9.1, `notification.show` answers
  `shown: true, reason: "shown"` whenever a client is attached, even with the
  default `[ui.toast] delivery = "off"`, and nothing appears. The viewing
  client applies the setting, not the server. paneMorph therefore reads
  `ui.toast.delivery` from herdr's config, read-only. When toasts are off, it
  opens its notice popup directly (7.6). With `delivery = "herdr"`, the toast
  appeared once the client had reported terminal focus.
- **⎋ latency.** The window itself exits 1 to 2 ms after ⎋ (pty harness, 5
  runs). Through herdr, total ⎋-to-closed time was 161 to 185 ms with herdr's
  mouse capture on, which is the default, and 32 to 45 ms with
  `[ui] mouse_capture = false`. herdr's client holds a lone ⎋ for up to
  150 ms while the host terminal captures the mouse
  (`MOUSE_ACTIVE_ESCAPE_SEQUENCE_FLUSH_TIMEOUT_MS`), otherwise 10 ms. The
  design's 35 ms figure holds only with mouse capture off. To avoid turning
  mouse capture on themselves, the windows request mouse reports only when
  herdr captures the mouse. ⌃C closes at once in every case.
- **Keys right after a window closes.** herdr routes keys to a popup until the
  attached client learns that it closed, a few milliseconds after the process
  exits. A key typed in that gap is dropped. The live tests wait for the redraw.

## Where paneMorph differs from the catalogue

| Row | Catalogue | paneMorph 0.2.0 | Why |
| --- | --- | --- | --- |
| 1.16 | Require herdr 0.9.1 | Keeps `min_herdr_version = "0.9.0"`, calls `pane.focus` after followed moves, and recommends 0.9.1 | 0.9.0 is still common; question 2 showed that the call works |
| 2.5 | Kitty flag for ⇧⏎ (catalogue) or no ⇧⏎ (approved design page) | No ⇧⏎. ⏎ on a tab row fetches the whole tab | Follows the approved design |
| 3.1 | Agent, else command, else folder | An idle shell (the pane's own shell at its prompt) is not a running command, so the folder names the tab | "zsh" was a poor tab name in live tests |
| 5.14 | Journal valid after a handoff, if terminal ids survive | They do not, so the journal goes stale and ⌃⌥Z says "Nothing to undo" | Question 8 |
| 6.2 | The popup process calls `pane.move`, then exits | The popup hands the move to a detached worker | Question 7: herdr kills the popup when its tab closes |
| 6.11 | ⎋ closes in 35 ms | 1 to 2 ms in the window; about 165 ms through herdr with mouse capture on | herdr's own ⎋ hold; see above |
| 6.18 | Mouse in the window | Only when herdr's `ui.mouse_capture` is on | Keeps ⎋ fast for keyboard-only setups |
| 7.6 | Trust `shown: false` | Decides from herdr's `ui.toast.delivery` | `shown` is always true when a client is attached |

## Not done

- No live herdr session was tested on Linux. The unit and pseudo-terminal
  suites run in CI on Ubuntu as well as macOS, and the code is
  platform-neutral Unix.
- The ⌃⌥ chords were not pressed on a physical keyboard in the working
  Ghostty (question 5), because that means the working session.
- Held-key repeats beyond herdr's 32 concurrent plugin commands (4.9) were not
  driven live. They queue behind the lock and stop at the edge, like fast
  presses.

## Row coverage

Every row, with its status in the catalogue and where paneMorph covers it.
"unit" names test functions (`cargo test`); "live" names herdr versions on
which a live check citing the row passed, or `live_herdr.rs`. Rows marked
*herdr* describe herdr behaviour that needs no paneMorph code.

| Row | Status | Situation | Covered by |
| --- | --- | --- | --- |
| 1.1 | Verified | To an existing tab in this space | unit: `edge_1_1_send_to_existing_tab`; live: 0.9.0, 0.9.1, live_herdr.rs |
| 1.2 | Decision | To a new tab ("New tab here" row, or ⌃⌥T) | unit: `edge_1_2_new_tab_here_is_placed_next_to_current_tab`; live: 0.9.0, 0.9.1, live_herdr.rs |
| 1.3 | Verified | To a tab in another space (→ on the space, then the tab) | unit: `edge_1_3_and_1_18_send_to_other_space`; live: 0.9.0, 0.9.1, live_herdr.rs |
| 1.4 | Decision | To "New tab in ‹space›" | unit: `edge_1_4_new_tab_in_other_space_at_end` |
| 1.5 | Decision | To a new space ("New space" row, or ⌃⌥N) | unit: `edge_1_5_3_3_3_8_new_space_named_after_folder_at_bottom`; live: 0.9.0, 0.9.1, live_herdr.rs |
| 1.6 | Verified | The pane is the last pane in its tab | unit: `edge_1_6_last_pane_closes_tab`; live: 0.9.0, 0.9.1, live_herdr.rs |
| 1.7 | Verified | The pane is the last pane in its space (sent to another space) | unit: `edge_1_7_last_pane_closes_space`, `edge_1_7_rows_warn_about_closing_space`, `edge_1_7_warns_before_closing_a_space`; live: 0.9.0, 0.9.1 |
| 1.8 | Verified | Into its own tab | unit: `edge_1_8_send_into_own_tab_is_refused` |
| 1.9 | Decision | A pane already alone in its tab, sent to "New tab here" | unit: `edge_1_9_4_5_4_6_noops_move_nothing`, `edge_1_9_and_4_5_alone_in_tab`, `edge_1_9_new_tab_here_disabled_when_alone` |
| 1.10 | Decision | The target tab is zoomed | unit: `edge_1_10_zoomed_target_is_unzoomed`; live: 0.9.0, 0.9.1 |
| 1.11 | Decision | The source tab is zoomed (you are sending the zoomed pane) | unit: `edge_1_11_and_5_16_zoomed_source_round_trip`; live: 0.9.0, 0.9.1, live_herdr.rs |
| 1.12 | Decision | The focused pane vs an unfocused pane | unit: `edge_1_12_source_pane_precedence`, `edge_1_12_moving_pane_does_not_follow_later_focus`; live: 0.9.0 (Q3) |
| 1.13 | Verified | Clicking another pane while Send is open | herdr: popups are session-modal |
| 1.14 | Verified | Right vs below, and the ratio | unit: `edge_1_14_send_below`, `edge_1_14_tab_toggles_split` |
| 1.15 | Verified | Where focus goes after a Send | unit: `edge_1_15_focus_follows_with_explicit_focus_call`; live: 0.9.0, 0.9.1 |
| 1.16 | Decision | herdr 0.9.0 does not move the window on `focus: true` | live: 0.9.0, 0.9.1 |
| 1.17 | Verified | A pane running an agent mid-turn | herdr: moves never restart a process |
| 1.18 | Verified | The pane id changes after a cross-space move | unit: `edge_1_3_and_1_18_send_to_other_space`; live: 0.9.0, 0.9.1 |
| 1.19 | Assumed | The focused pane is another plugin's pane (split, tab or overlay) | live: 0.9.0, 0.9.1 |
| 2.1 | Verified | A pane from the current tab | unit: `edge_2_1_and_2_15_fetch_refuses_current_tab` |
| 2.2 | Verified | A pane from another tab in this space | unit: `edge_2_2_fetch_pane_from_this_space`; live: 0.9.0, 0.9.1, live_herdr.rs |
| 2.3 | Verified | A pane from another space | unit: `edge_2_3_and_2_7_fetch_from_other_space_closes_it` |
| 2.4 | Verified | A whole tab, with its splits (⇧⏎, or ⏎ on a tab row) | unit: `edge_2_4_fetch_whole_tab_rebuilds_splits`; live: 0.9.0, 0.9.1, live_herdr.rs |
| 2.5 | Decision (Assumed) | ⇧⏎ arrives as a plain ⏎ | `fetch_enter_on_tab_row_fetches_whole_tab_and_pane_row_fetches_pane` |
| 2.6 | Verified | A whole tab from another space | unit: `edge_2_6_fetch_whole_tab_across_spaces` |
| 2.7 | Verified | The only pane of a tab | unit: `edge_2_3_and_2_7_fetch_from_other_space_closes_it` |
| 2.8 | Decision | Fetching into a zoomed tab (your tab is zoomed) | unit: `edge_2_8_fetch_into_zoomed_tab` |
| 2.9 | Decision | Fetching from a zoomed tab elsewhere | unit: `edge_2_9_fetch_from_zoomed_tab_rezooms_it` |
| 2.10 | Verified | The pane closes while the window is open | unit: `edge_2_10_fetch_closed_pane` |
| 2.11 | Decision | Many panes, and filtering | unit: `edge_2_11_fetch_filter_words` |
| 2.12 | Decision | Commands for panes without an agent | live: 0.9.0, 0.9.1, live_herdr.rs |
| 2.13 | Decision | Untitled panes, and naming fallbacks | unit: `edge_2_13_command_names_a_pane_without_agent`, `edge_2_13_field_precedence`, `edge_2_13_untitled_pane_shows_shell_and_folder_not_id`; live: 0.9.0, 0.9.1 |
| 2.14 | Verified | Tab and space names in the list | unit: `edge_2_14_duplicate_space_labels_get_numbers` |
| 2.15 | Decision | Fetching your current tab as a whole | unit: `edge_2_1_and_2_15_fetch_refuses_current_tab` |
| 2.16 | Verified | A whole tab with ⇥ set to below | unit: `edge_2_16_below_only_changes_outer_split`, `edge_2_16_whole_tab_below` |
| 3.1 | Decision | Naming a new tab | unit: `edge_3_1_new_tab_name_rule`, `edge_3_1_new_tab_named_after_program`; live: 0.9.0, 0.9.1 |
| 3.2 | Decision | The tab name is already used in that space | unit: `edge_3_2_and_3_4_numeric_suffixes`, `edge_3_2_duplicate_tab_name_gets_suffix` |
| 3.3 | Decision | Naming a new space | unit: `edge_1_5_3_3_3_8_new_space_named_after_folder_at_bottom`, `edge_3_3_space_named_after_folder`; live: 0.9.0, 0.9.1 |
| 3.4 | Decision (Assumed) | The space name is already used | unit: `edge_3_2_and_3_4_numeric_suffixes`, `edge_3_4_duplicate_space_name_gets_suffix` |
| 3.5 | Decision | Cleaning up a name | unit: `edge_3_5_sanitize` |
| 3.6 | Decision | Where a new tab appears | live: 0.9.0, 0.9.1 |
| 3.7 | Decision | The follow-up `tab.move` fails | unit: `edge_3_7_tab_move_failure_keeps_the_move_and_warns` |
| 3.8 | Verified | Where a new space appears | unit: `edge_1_5_3_3_3_8_new_space_named_after_folder_at_bottom` |
| 3.9 | Verified | The new space's folder and git status | herdr: identity from the moved terminal's cwd |
| 3.10 | Decision | herdr's own naming prompts | unit: `edge_3_10_names_are_passed_never_prompted` |
| 3.11 | Verified | An empty source tab or space afterwards | herdr closes emptied tabs and spaces (see 1.6, 1.7) |
| 3.12 | Verified | Untitled tabs renumber | unit: `edge_3_12_untitled_tab_detection_uses_position` |
| 4.1 | Decision | ⌃⌥→ on the last tab | unit: `edge_4_1_to_4_3_neighbours_follow_bar_order_without_wrap`; live: 0.9.0, 0.9.1, live_herdr.rs |
| 4.2 | Decision | ⌃⌥← on the first tab | unit: `edge_4_1_to_4_3_neighbours_follow_bar_order_without_wrap`; live: live_herdr.rs |
| 4.3 | Verified | Which tab is "next" | unit: `edge_4_1_to_4_3_neighbours_follow_bar_order_without_wrap`, `edge_4_3_order_ignores_tab_numbers` |
| 4.4 | Decision | Only one tab in the space | unit: `edge_4_4_only_one_tab` |
| 4.5 | Decision | ⌃⌥T when the pane is already alone in its tab | unit: `edge_1_9_4_5_4_6_noops_move_nothing`, `edge_1_9_and_4_5_alone_in_tab`; live: 0.9.0, 0.9.1, live_herdr.rs |
| 4.6 | Decision | ⌃⌥N when the pane is alone in its space | unit: `edge_1_9_4_5_4_6_noops_move_nothing`, `edge_4_6_alone_in_space`; live: 0.9.0, 0.9.1 |
| 4.7 | Verified | ⌃⌥N when the pane is alone in its tab, but the space has other tabs | unit: `edge_4_7_alone_in_tab_can_go_to_new_space` |
| 4.8 | Decision | A key pressed twice fast | unit: `edge_4_8_and_7_12_queue_lock_serialises_and_releases`, `edge_4_8_repeated_next_moves_two_tabs`; live: 0.9.0, 0.9.1, live_herdr.rs |
| 4.9 | Verified | A key held down | lock queue (`edge_4_8_and_7_12_queue_lock_serialises_and_releases`); herdr caps 32 commands |
| 4.10 | Decision | A quick key pressed while a paneMorph window is open | unit: `edge_4_10_and_6_12_chords`, `edge_4_10_ctrl_alt_chords_inside_a_window`, `edge_4_10_own_key_ignored_other_switches`; live: 0.9.0, 0.9.1 |
| 4.11 | Verified | A key pressed while another popup (codeMap, a herdr popup command) is open | herdr: keys go to the open popup |
| 4.12 | Verified | A key pressed while the window is still opening | herdr: keys dropped while a popup opens |
| 4.13 | Verified | A ⌃⌥ chord the OS or terminal takes | README: chord conflicts |
| 4.14 | Assumed | ⌃⌥Z is also bound to herdr's zoom | README warning; question 9 (live) |
| 4.15 | Decision | Old ctrl+e, ctrl+s or ctrl+f bindings left in config | unit: `edge_4_15_and_6_7_manifest_keeps_aliases_and_min_version`; live: live_herdr.rs |
| 4.16 | Decision | ⌃⌥M | unit: `edge_4_16_ctrl_alt_m_is_ignored` |
| 4.17 | Decision | A quick key on a zoomed tab | unit: `edge_4_17_quick_key_on_zoomed_tab`; live: 0.9.0, 0.9.1 |
| 5.1 | Decision | What is remembered | unit: `edge_5_1_journal_entry_holds_what_undo_needs` |
| 5.2 | Decision | How long, and how many | unit: `edge_5_2_keeps_last_twenty` |
| 5.3 | Decision | Pressing ⌃⌥Z again | unit: `edge_5_3_each_undo_takes_the_next_older_move`, `edge_5_3_undo_chain`; live: 0.9.0, 0.9.1 |
| 5.4 | Decision | Where focus goes on undo | unit: `edge_5_4_undo_after_fetch_you_stay`, `edge_5_4_undo_after_send_follows_the_pane`; live: 0.9.0, 0.9.1 |
| 5.5 | Decision | Putting the pane back exactly | unit: `edge_5_5_first_child_returns_with_swap`, `edge_5_5_group_neighbour_uses_first_pane_and_is_not_exact`, `edge_5_5_second_child_returns_without_swap`, `edge_5_5_undo_first_child_swaps_into_place`, `edge_5_5_undo_restores_exact_position`; live: 0.9.0, 0.9.1, live_herdr.rs |
| 5.6 | Decision | The former neighbour is gone | unit: `edge_5_6_neighbour_gone`, `edge_5_6_no_neighbour_left` |
| 5.7 | Decision | The move had closed the source tab | unit: `edge_5_7_undo_recreates_closed_tab_in_place`; live: 0.9.0, 0.9.1, live_herdr.rs |
| 5.8 | Decision (Assumed) | The move had closed the source space | unit: `edge_5_8_and_5_9_undo_recreates_closed_space`; live: 0.9.0, 0.9.1 |
| 5.9 | Verified | Undo of a cross-space move (the pane id changed) | unit: `edge_5_8_and_5_9_undo_recreates_closed_space`; live: 0.9.0, 0.9.1 |
| 5.10 | Decision | Undo of a whole-tab fetch | unit: `edge_5_10_undo_whole_tab_fetch`; live: 0.9.0, 0.9.1 |
| 5.11 | Decision | The destination tab was closed, or the pane itself was closed | unit: `edge_5_11_undo_closed_pane` |
| 5.12 | Decision | The pane was moved again by other means (mouse, herdr keys, another plugin) | unit: `edge_5_12_undo_stale_after_manual_move` |
| 5.13 | Verified | Undo after herdr restarts | unit: `edge_5_13_restart_clears_journal` |
| 5.14 | Assumed | Undo after a live handoff | question 8 (live): the journal goes stale |
| 5.15 | Decision | Two named sessions | unit: `edge_5_15_sessions_do_not_share_a_journal` |
| 5.16 | Decision | The tab the pane returns to is zoomed | unit: `edge_1_11_and_5_16_zoomed_source_round_trip`; live: 0.9.0, 0.9.1, live_herdr.rs |
| 5.17 | Decision | Nothing to undo | unit: `edge_5_17_nothing_to_undo` |
| 5.18 | Decision | A whole-tab fetch that partly failed | unit: `edge_5_18_partial_whole_tab_entry_returns_moved_panes` |
| 6.1 | Verified | How the windows open | unit: `edge_6_1_popup_sizes_follow_the_spec`; live: 0.9.0, 0.9.1, live_herdr.rs |
| 6.2 | Verified | The window makes the move itself | detached worker (`src/ui/apply.rs`); live "1.6 via window" |
| 6.3 | Verified | Opened twice | unit: `edge_6_3_and_6_4_ui_busy_is_recognised`; live: live_herdr.rs |
| 6.4 | Verified | Settings, copy mode or another herdr modal is open | unit: `edge_6_3_and_6_4_ui_busy_is_recognised` |
| 6.5 | Decision | The herdr socket is unavailable | unit: `edge_6_5_window_without_herdr_says_so_and_waits` |
| 6.6 | Decision | A move whose reply was lost | unit: `edge_6_6_lost_reply_is_verified` |
| 6.7 | Verified | herdr is older than paneMorph needs | unit: `edge_4_15_and_6_7_manifest_keeps_aliases_and_min_version`, `edge_6_7_version_gate` |
| 6.8 | Verified | Upgrading from herdr 0.9.0 with Homebrew | README: Herdr versions |
| 6.9 | Decision | A small terminal | unit: `edge_6_9_small_terminals` |
| 6.10 | Verified | The terminal is too small to open a popup at all | unit: `edge_6_10_too_small_for_a_popup` |
| 6.11 | Verified | Esc latency | unit: `edge_6_11_escape_closes_under_100ms`; live: 0.9.0, 0.9.1, live_herdr.rs |
| 6.12 | Verified | ⎋ versus a ⌃⌥ chord | unit: `edge_4_10_and_6_12_chords` |
| 6.13 | Decision | ⌃C in the window | unit: `edge_6_13_ctrl_c_closes`, `edge_6_13_ctrl_c_closes_without_moving` |
| 6.14 | Verified | The terminal is resized while the window is open | unit: `edge_6_14_resize_keeps_filter`, `edge_6_14_resize_keeps_state` |
| 6.15 | Decision | The list goes stale while open | unit: `edge_6_15_selection_survives_refresh`; live: 0.9.0, 0.9.1 |
| 6.16 | Decision | No other tabs or spaces exist | unit: `edge_6_16_fetch_nothing_to_fetch`, `edge_6_16_nothing_to_fetch`, `edge_6_16_send_hides_other_spaces_when_none` |
| 6.17 | Decision | Filter keys | unit: `edge_6_17_filter_keys` |
| 6.18 | Decision | The mouse in the window | unit: `edge_6_18_double_click_acts`, `edge_6_18_mouse_click_and_double_click` |
| 6.19 | Assumed | The window's title | question 6 (live) |
| 6.20 | Decision | Focus leaves the tab before the window exits | unit: `edge_6_20_late_problems_are_returned_to_the_worker` |
| 7.1 | Decision | A whole-tab move fails midway | unit: `edge_7_1_return_order_restores_three_pane_tab_exactly`, `edge_7_1_whole_tab_failure_rolls_back_exactly` |
| 7.2 | Decision | The rollback fails too | unit: `edge_7_2_rollback_failure_keeps_journal_entry` |
| 7.3 | Verified | herdr's own recovery after `pane_move_failed` | unit: `edge_7_3_rollback_finds_a_pane_herdr_recovered` |
| 7.4 | Verified | herdr reports an unchanged move | unit: `edge_7_4_unchanged_move_reason_in_plain_words` |
| 7.5 | Decision | Showing errors in a window | unit: `edge_7_5_inline_error_in_footer` |
| 7.6 | Decision | Showing errors from quick keys | unit: `edge_7_6_notice_shows_title_message_and_hint`; live: 0.9.0, 0.9.1 |
| 7.7 | Verified | The notice popup cannot open | live: 0.9.0, 0.9.1 |
| 7.8 | Decision | Logging | unit: `edge_7_8_log_keeps_last_500_lines` |
| 7.9 | Decision | What a log line holds | unit: `edge_7_9_log_line_contents` |
| 7.10 | Decision | Action exit codes | live: 0.9.0, 0.9.1, live_herdr.rs |
| 7.11 | Decision | paneMorph never destroys terminals | unit: `edge_7_11_never_destroys_terminals`; live: 0.9.0, 0.9.1, live_herdr.rs |
| 7.12 | Verified | A paneMorph process crashes while holding the queue lock | unit: `edge_4_8_and_7_12_queue_lock_serialises_and_releases` |
| 7.13 | Assumed | python3 or curses is missing | question 11 (live); `panemorph doctor` |
| 7.14 | Verified | The plugin is disabled | herdr: `plugin_disabled` |
