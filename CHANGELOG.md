# Changelog

All notable changes to paneMorph are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and paneMorph adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-09-27

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
- Prebuilt release archives for macOS (Apple silicon and Intel) and Linux
  (x86_64 and arm64, statically linked), each a ready-to-link plugin
  directory with a SHA-256 checksum.
- `cargo run --example screenshots` renders the README screenshots from the
  window code; `--live` captures a real herdr client in a throwaway session.

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

## [0.1.2] - 2026-09-21

### Fixed

- Close the selector and restore herdr's layout before applying a move, so
  send and bring actions are no longer blocked by the overlay's temporary
  zoom state.
- Keep the action running until the move completes, so failures reach the
  plugin action log instead of a premature success entry.

### Added

- Isolated herdr integration coverage for selection, movement, cancellation
  and preservation of terminal identities.

## [0.1.1] - 2026-09-21

### Fixed

- Selector overlays now target herdr's active pane implicitly.

### Changed

- The conflicting `Ctrl+G` and `Ctrl+R` bindings are now `Ctrl+S` and
  `Ctrl+F`.

## [0.1.0] - 2026-09-20

### Added

- A two-key preset for tab and pane creation and navigation.
- Focused-pane extraction to a background tab.
- A searchable destination selector for sending a pane to another tab.
- A searchable tab and pane selector for importing live panes on the right.
- Multi-pane source layouts preserved through ordered live pane moves.
- Preflight zoom checks, partial-move rollback and local diagnostics.

[Unreleased]: https://github.com/Jenish-Shobhit/paneMorph/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/Jenish-Shobhit/paneMorph/releases/tag/v0.2.0
[0.1.2]: https://github.com/Jenish-Shobhit/paneMorph/tree/b8b645e
[0.1.1]: https://github.com/Jenish-Shobhit/paneMorph/tree/0523ebb
[0.1.0]: https://github.com/Jenish-Shobhit/paneMorph/tree/14f2507
