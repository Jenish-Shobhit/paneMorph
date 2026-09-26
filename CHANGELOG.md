# Changelog

All notable changes to paneMorph will be documented here.

## 0.2.0 — 2026-09-27

paneMorph is now a Rust binary (ratatui and crossterm) built with
`cargo build --release`. The Python package is gone. herdr 0.9.0 still works,
and 0.9.1 is recommended.

### Added

- **Send (⌃⌥S).** A popup, 60% × 50%, that sends the focused pane to any tab,
  a new tab next to this one, a tab or new tab in another space, or a new
  space. Nothing zooms.
- **Fetch (⌃⌥F).** A popup, 64% × 60%, that lists every pane in every space
  by name. ⏎ fetches a pane, or a whole tab with its splits. ⌃⌥S and ⌃⌥F
  switch between the two windows.
- **Quick keys.** `move-tab-new` (⌃⌥T), `move-space-new` (⌃⌥N) and
  `move-tab-prev` / `move-tab-next` (⌃⌥← / ⌃⌥→, in tab-bar order, with no
  wrap).
- **Undo (⌃⌥Z).** Keeps a journal of the last 20 moves per herdr session,
  tracked by terminal id. Undo puts a pane back beside its old neighbour at
  its old ratio, recreates closed tabs and spaces in their old places, and
  zooms a pane again if paneMorph had unzoomed it.
- **Naming.** New tabs are named after the pane's agent, its running command
  or its folder; new spaces after its folder. A name already in use gets
  " 2", " 3".
- **Zoom.** Zoomed tabs are unzoomed before a move. The tab you look at stays
  unzoomed, and a tab elsewhere is zoomed again.
- **Fast presses** queue behind a lock file, so each acts on the state the
  one before left.
- **Errors.** They appear inline in the windows. A failed quick key shows a
  herdr toast, or a small notice popup when herdr's toasts are off (the
  default). `panemorph.log` in the plugin state directory keeps the last
  500 lines.
- `panemorph doctor` checks the connection and versions.
  `panemorph preview send|fetch` shows a window over a simulated session.

### Changed

- The windows are herdr popups, not a zoomed overlay. ⎋ closes them at once;
  herdr itself holds a lone ⎋ for up to 150 ms while it captures the mouse.
- Rows show status, agent or command, title and folder, never raw pane ids.
- Every move made from a window runs in a short-lived background worker, so
  it completes even when herdr closes the popup, as it does when you send the
  last pane of the popup's own tab.
- On herdr 0.9.0, paneMorph calls `pane.focus` after a move that should take
  you along, because 0.9.0 does not move your view by itself (herdr #4153).
- `config/panemorph-keys.toml` now holds the seven ⌃⌥ bindings. The v0.1
  two-key preset for herdr's built-in actions is no longer shipped.

### Deprecated

- `extract-pane`, `send-pane` and `bring-pane` remain as aliases for
  `move-tab-new`, `send` and `fetch`, and log a one-line deprecation note.
  They will be removed in v1.0.

### Removed

- The Python package, the overlay selector, its temporary result file and
  its polling loop.

## 0.1.2 — 2026-09-21

- Close the selector and restore Herdr's layout before applying a move, fixing
  send and bring actions blocked by the overlay's temporary zoom state.
- Keep the action running until the move completes, so failures reach the
  plugin action log instead of a prematurely successful launch entry.
- Add isolated Herdr integration coverage for selection, movement, cancellation,
  and preservation of terminal identities.

## 0.1.1 — 2026-09-21

- Fix selector overlays by targeting Herdr's active pane implicitly.
- Replace conflicting `Ctrl+G` and `Ctrl+R` bindings with `Ctrl+S` and `Ctrl+F`.

## 0.1.0 — 2026-09-20

- Add direct two-key preset for tab and pane creation and navigation.
- Add focused-pane extraction to a background tab.
- Add searchable destination selector for sending a pane to another tab.
- Add searchable tab/pane selector for importing live panes on the right.
- Preserve multi-pane source layouts through ordered live pane moves.
- Add preflight zoom checks, partial-move rollback, and local diagnostics.
