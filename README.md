# paneMorph

**Send any pane anywhere. Fetch any pane here. Keep your terminals running.**

paneMorph is an open-source [Herdr](https://herdr.dev) plugin for rearranging
live terminal workspaces. Send the pane you are in to another tab, a new tab,
another space or a new space. Fetch any pane, or a whole tab with its splits,
from any space into the tab you are in. Every move keeps the process, its
scrollback and its working directory. ⌃⌥Z puts the last move back.

[Install](#install) · [Keys](#keys) · [Send](#send) · [Fetch](#fetch) ·
[Quick keys](#quick-keys) · [Undo](#undo) · [Troubleshooting](#troubleshooting)
· [Development](#development)

## Install

You need **Herdr 0.9.0 or newer** (0.9.1 recommended, see
[Herdr versions](#herdr-versions)), **macOS or Linux**, and a **Rust toolchain**
(`cargo`, 1.89 or newer) to build the plugin once.

From GitHub, once v0.2.0 is tagged:

```sh
herdr plugin install Jenish-Shobhit/paneMorph --ref v0.2.0
```

`plugin install` runs the manifest's build command, `cargo build --release
--locked`, before it registers the plugin, so `cargo` must be on your `PATH`.

From a local checkout:

```sh
git clone https://github.com/Jenish-Shobhit/paneMorph.git
cd paneMorph
cargo build --release
herdr plugin link "$PWD" --enabled
```

Every action runs `./bin/panemorph`, a small launcher that starts
`target/release/panemorph`. herdr resolves a command that starts with `./`
against the plugin directory, for actions and for popup windows alike.

**Picking up changes.** herdr re-reads the manifest of every installed or
linked plugin from disk each time a key or window invokes it, so there is
nothing to relink or restart. After `git pull` (or switching branches) in a
linked checkout, run `cargo build --release`. Until the build finishes, the
keys report "paneMorph is not built" in `herdr plugin log list`.

paneMorph has no telemetry and talks only to herdr's local socket.

## Keys

Installing the plugin does not bind any keys. Merge these blocks into
`~/.config/herdr/config.toml` (also in
[config/panemorph-keys.toml](config/panemorph-keys.toml)), then run
`herdr config check` and `herdr server reload-config`:

```toml
# ~/.config/herdr/config.toml, paneMorph keys
[[keys.command]]
key = "ctrl+alt+s"
type = "plugin_action"
command = "dev.panemorph.send"
description = "Send pane"

[[keys.command]]
key = "ctrl+alt+f"
type = "plugin_action"
command = "dev.panemorph.fetch"
description = "Fetch a pane here"

[[keys.command]]
key = "ctrl+alt+t"
type = "plugin_action"
command = "dev.panemorph.move-tab-new"
description = "Pane to a new tab"

[[keys.command]]
key = "ctrl+alt+n"
type = "plugin_action"
command = "dev.panemorph.move-space-new"
description = "Pane to a new space"

[[keys.command]]
key = "ctrl+alt+left"
type = "plugin_action"
command = "dev.panemorph.move-tab-prev"
description = "Pane one tab left"

[[keys.command]]
key = "ctrl+alt+right"
type = "plugin_action"
command = "dev.panemorph.move-tab-next"
description = "Pane one tab right"

[[keys.command]]
key = "ctrl+alt+z"
type = "plugin_action"
command = "dev.panemorph.undo"
description = "Undo the last move"
```

| Key | Action | What it does |
| --- | --- | --- |
| ⌃⌥S | `send` | Open the Send window for the focused pane |
| ⌃⌥F | `fetch` | Open the Fetch window |
| ⌃⌥T | `move-tab-new` | Move the pane to a new tab next to this one |
| ⌃⌥N | `move-space-new` | Move the pane to a new space named after its folder |
| ⌃⌥← / ⌃⌥→ | `move-tab-prev` / `move-tab-next` | Move the pane one tab left or right |
| ⌃⌥Z | `undo` | Put the last moved pane back |

**Old bindings keep working.** v0.1 bound `ctrl+e`, `ctrl+s` and `ctrl+f` to
`extract-pane`, `send-pane` and `bring-pane`. In v0.2 those ids are aliases for
`move-tab-new`, `send` and `fetch`. Each use writes a one-line deprecation note
to the plugin log. The aliases go away in v1.0, so switch to the new ids when
convenient.

**Chord conflicts.**

- `herdr config check` reports a clash when herdr's own `zoom` is also on
  `ctrl+alt+z` (the Keyboard doc's prefix-free example binds it there). herdr
  keeps `keys.zoom` and disables the command, so keep only one.
- On Linux, GNOME and some terminals take `ctrl+alt+arrows`, Ubuntu and Fedora
  take `ctrl+alt+t`, and Konsole takes `ctrl+alt+s`. Rebind those in the
  snippet if they never reach herdr.
- paneMorph never binds ⌃⌥M.

## Send

Press ⌃⌥S. A window opens over herdr as a popup; nothing zooms and the layout
stays visible underneath.

```text
┌paneMorph · send──────────────────────────────────────────────────┐
│ moving ○ claude  portfolio copy  acme-web                        │
│ › filter tabs and spaces                                         │
│                                                                  │
│ new                                                              │
│▌ ＋ New tab here   named claude, next to this tab              ⏎ │
│  ＋ New space      named acme-web                                │
│ this space · Studio                                              │
│     Landing_Page_Copy                                       here │
│     API refactor   1 pane                                        │
│     Load tests     1 pane                                        │
│ other spaces                                                     │
│     Payments_Service_Rewrite                                   → │
│ ⏎ send   → open space   ⇥ right/below   ⎋ close     lands: right │
└──────────────────────────────────────────────────────────────────┘
```

- The top line names the pane that moves: its status, agent or running
  command, title and folder.
- Type to filter every list at once.
- **New tab here** puts the pane in a new tab right after this one. **New
  space** puts it in a new space at the bottom of the sidebar. Names come from
  the pane: its agent, else its running command, else its folder. A name
  already in use gets " 2", " 3".
- Your current tab is dimmed and marked **here**.
- ⏎ or → on another space opens it and lists its tabs plus **New tab in
  ‹space›**. ← goes back.
- ⇥ switches where the pane lands: right of, or below, the target tab's
  focused pane.
- Focus goes with the pane.

## Fetch

Press ⌃⌥F to list every pane in every space, grouped space → tab → pane, by
name (never by raw id). Panes without an agent show their running command,
such as `python3 scrape_docs.py`.

- ⏎ on a **pane** row brings that pane beside yours.
- ⏎ on a **tab** row brings the whole tab and rebuilds its splits beside
  yours. The emptied tab closes.
- ⇥ switches right or below. You stay on your own pane.
- Inside either window, ⌃⌥S and ⌃⌥F switch between Send and Fetch.
- ⎋ or ⌃C closes without moving anything. Errors appear in the window's footer
  and the window stays open.

## Quick keys

⌃⌥T, ⌃⌥N, ⌃⌥← and ⌃⌥→ move the focused pane without a window.

- Nothing wraps: ⌃⌥→ on the last tab says "Already the last tab".
- ⌃⌥T on a pane that is already alone in its tab does nothing, and so does
  ⌃⌥N on a pane alone in its space.
- Fast presses queue behind a lock file, so ⌃⌥→ twice moves the pane two tabs
  over.
- Zoomed tabs are unzoomed before a move. The tab you look at stays
  unzoomed, and a tab elsewhere is zoomed again.

## Undo

⌃⌥Z puts the last move back: beside the same neighbour, at the same ratio and
on the same side. It recreates a tab or space the move had closed, with its
old name and position, and zooms a pane again if paneMorph had unzoomed it.
Each press undoes the next older move.

- The journal keeps the last 20 moves of each herdr session, tracked by
  terminal id.
- A move goes stale once its pane is closed or moved by other means. Undo
  then says so and drops that entry.
- The journal goes stale after a herdr restart or a live handoff, because both
  give terminals new ids.

## How moves work

paneMorph moves live panes with herdr's `pane.move`, and only ever calls
`pane.move`, `pane.zoom`, `pane.swap`, `pane.focus`, `tab.move` and
`workspace.move`. It never calls `layout.apply`, `pane.close`, `tab.close` or
`workspace.close`, so a move never restarts a terminal.

- **Whole-tab fetches** replay the tab's split tree with ordered moves. If a
  step fails, the panes already moved go back to their exact places.
- **Moves from a window.** The window hands the move to a short-lived
  background worker. herdr closes a popup when the tab it opened on closes,
  which happens when you send a tab's last pane. The worker finishes the move,
  the undo journal and the log regardless.
- **Logs.** Actions print one line per move or error, visible with
  `herdr plugin log list`. Windows also append to `panemorph.log` in the
  plugin state directory (the last 500 lines).
- **Toasts.** herdr's default is `[ui.toast] delivery = "off"`. When toasts
  are off, a failed quick key opens a small notice popup that closes on any
  key or after 4 seconds.

## Herdr versions

paneMorph runs on **herdr 0.9.0** and recommends **0.9.1**.

On 0.9.0, a move that asks herdr to follow the pane does not move your view
(herdr issue #4153). paneMorph works around this by calling `pane.focus` right
after the move. Measured on 0.9.0, the view then follows within 10–25 ms,
against about 10 ms on 0.9.1. A key pressed inside that window can still act on
the pane you left. See [docs/verification.md](docs/verification.md).

To upgrade with Homebrew:

```sh
brew upgrade herdr
herdr server stop     # stops every pane process; agents with herdr integrations resume
herdr
```

A client-only update leaves the old server running, and Homebrew installs
cannot hand off live.

## Troubleshooting

**A key does nothing.** Check that herdr has focus and that the snippet was
merged and reloaded. Then look at `herdr plugin log list`. To rule out the
key binding, run an action directly inside herdr:

```sh
herdr plugin action invoke send --plugin dev.panemorph
```

**"paneMorph is not built".** Run `cargo build --release` in the plugin
directory.

**Check the connection and versions.** Inside herdr, from the plugin
directory:

```sh
./bin/panemorph doctor
```

This read-only check reports the herdr version, the topology counts and the
undo journal.

**⎋ feels slow.** With herdr's mouse support on (the default), herdr's client
holds a lone ⎋ for up to 150 ms to tell it apart from a mouse report, so the
window closes about 160 ms after the key. With `[ui] mouse_capture = false`,
it closes in about 40 ms. ⌃C always closes at once.

**A window closes after an error.** It should not: errors stay in the footer
until ⎋. Quick-key errors go to the plugin log and a toast or notice popup.

## Development

```sh
cargo build --release
cargo test                    # unit, rendering (ratatui TestBackend) and pty tests
cargo clippy --all-targets
cargo fmt --check
```

- `cargo run -- preview send` (or `fetch`) opens a window over a simulated
  session, with no herdr needed.
- `tests/pty_windows.rs` drives the real binary through a pseudo-terminal and
  checks that ⎋ closes the window in under 100 ms.

The live test starts its own **isolated** herdr server. It uses a private
`XDG_CONFIG_HOME`, registers this checkout in that private registry only, and
refuses any socket outside its private directory:

```sh
cargo build --release
PANEMORPH_LIVE=1 cargo test --release --test live_herdr -- --ignored --nocapture
# another herdr binary: HERDR_BIN=/path/to/herdr
```

## Contributing and license

Bug reports and focused pull requests are welcome. For a bug report, include
your herdr and paneMorph versions, the action you tried, and the error from
`herdr plugin log list`. Remove private paths or terminal content first.

Read [CONTRIBUTING.md](CONTRIBUTING.md),
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) and [SECURITY.md](SECURITY.md).
paneMorph is released under the [MIT license](LICENSE).
