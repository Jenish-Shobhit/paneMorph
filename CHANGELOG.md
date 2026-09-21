# Changelog

All notable changes to paneMorph will be documented here.

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
