# paneMorph

**Turn a pane into a tab. Bring a tab back as a pane. Keep your terminals running.**

paneMorph is an open-source [Herdr](https://herdr.dev) plugin for rearranging live
terminal workspaces. Pull a running server beside your editor, send an agent to
its own tab, or bring an entire multi-pane layout into the tab you're using.

Three plugin actions handle the moves. An optional shortcut preset adds quick
tab creation, splitting, and navigation using Herdr's built-in commands. Every
shortcut is a direct two-key chord—there is no prefix sequence to enter first.

[Install](#install) · [Workflows](#workflows) · [Shortcuts](#shortcuts) ·
[Troubleshooting](#troubleshooting) · [Contribute](CONTRIBUTING.md)

## Workflows

The shortcuts below assume you have installed the [key preset](#configure-shortcuts).
A **tab** contains one or more **panes**; a pane is a running terminal.

### Create a tab or pane

Press **Ctrl+T** to create and focus a new tab. Press **Ctrl+N** to split a new
pane to the right. These are Herdr's native actions, included in the preset.

### Make the current pane a tab

Focus the pane and press **Ctrl+E**. It moves into a new background tab with its
running process intact. If the original tab still has other panes, you stay
there. If you move its only pane, Herdr removes the empty tab and resolves focus.

```text
Before:  Tab A [editor | server]
After:   Tab A [editor]          Tab B [server]
```

### Send the current pane to another tab

Press **Ctrl+S**, choose a destination tab, and press **Enter**. The current pane
moves to the right of that tab's focused pane. Choose **New background tab**
instead to extract it into a fresh tab.

### Bring a tab or pane into the current tab

Focus the pane you want to place things beside, then press **Ctrl+F**:

- Choose a **tab row** to bring its entire layout to the right of your pane.
- Choose an **indented pane row** to bring only that terminal.

For example, bringing Tab B into Tab A gives you:

```text
Before:                         After:
Tab A                           Tab A
┌─────────────┐                 ┌─────────────┬─────────────┐
│ editor      │                 │ editor      │ server      │
└─────────────┘                 │             ├─────────────┤
Tab B                           │             │ tests       │
┌─────────────┐                 └─────────────┴─────────────┘
│ server      │
├─────────────┤                 Tab B is removed once empty.
│ tests       │
└─────────────┘
```

Whole-tab moves preserve the source layout's internal split directions and
ratios. Moving a single pane leaves the source tab's other panes in place.

### Use the selector

Type to filter, use **↑ / ↓** to select, and press **Enter** to move. **Esc**
cancels. Both selectors only list destinations or sources in the current
workspace. The bring selector lists other tabs and their panes; it does not
rearrange panes already in the current tab.

## Install

You need **Herdr 0.9.0+**, **Python 3.10+** with the standard-library `curses`
module, and **macOS or Linux**. `python3` must be available to Herdr.

Run inside Herdr:

```sh
herdr plugin install Jenish-Shobhit/paneMorph
herdr plugin enable dev.panemorph
```

No additional Python packages are required. paneMorph has no telemetry or
external service; runtime communication stays on Herdr's local socket.

### Configure shortcuts

Installing the plugin does **not** automatically install its shortcuts.
Merge [config/panemorph-keys.toml](config/panemorph-keys.toml) into
`~/.config/herdr/config.toml`:

1. Add the preset's built-in bindings to your existing `[keys]` table. Do not
   create a second `[keys]` table or duplicate an existing key.
2. Add the three `[[keys.command]]` blocks for paneMorph. Replace any older
   paneMorph bindings you already have.
3. Validate and reload:

```sh
herdr config check
herdr server reload-config
```

You can keep just the three plugin bindings if you already have creation and
navigation shortcuts you like. Your existing prefix binding can stay as it is.

## Shortcuts

| Shortcut | Action | Provided by |
| --- | --- | --- |
| `Ctrl+T` | Create and focus a new tab | Herdr |
| `Ctrl+N` | Create a pane on the right | Herdr |
| `Ctrl+]` / `Ctrl+[` | Next / previous tab | Herdr |
| `Ctrl+L` / `Ctrl+H` | Next / previous pane | Herdr |
| Ctrl + backtick | Return to the last active pane | Herdr |
| `Ctrl+E` | Extract the focused pane to a background tab | paneMorph |
| `Ctrl+S` | Send the focused pane to a selected tab | paneMorph |
| `Ctrl+F` | Fetch a tab or pane into this tab on the right | paneMorph |

These bindings apply while Herdr has keyboard focus. They replace some familiar
terminal shortcuts: `Ctrl+S` is commonly flow control, `Ctrl+F` moves the cursor
forward, and `Ctrl+L` clears the screen. You can change every binding in the
preset. A shortcut intercepted by your operating system or terminal application
will not reach Herdr.

## How moves work

paneMorph uses Herdr's live `pane.move` API to retain terminal processes, PTYs,
scrollback, and working directories. Whole-tab imports read the source split
tree and reconstruct it with ordered pane moves; they do not recreate terminals
with `layout.apply`.

The selector closes before the move runs. This lets Herdr restore the tab from
the selector's temporary zoom state. The action remains running until selection
and movement finish, so its log reflects the completed operation.

Current limits:

- Moves stay within one workspace.
- Source and destination tabs must be unzoomed before a move. The selector's
  own temporary zoom is handled automatically.
- Whole-tab moves use multiple operations. If a later operation fails,
  paneMorph attempts to return moved panes to the source tab. Recovery is best
  effort and does not promise to restore the original split geometry.

See the [changelog](CHANGELOG.md) for fixes and the [original PRD](PRD.html) for
the design and workflow examples. This README describes the shipped behavior.

## Troubleshooting

**A shortcut does nothing or opens something else**

Check that Herdr has focus, that the shortcut preset was merged, and that the
configuration was reloaded. Then check registration and recent action logs:

```sh
herdr plugin list
herdr plugin action list
herdr plugin log list
```

To test the action independently of its shortcut, run either command inside
Herdr. It opens a selector; press **Esc** to cancel without moving anything.

```sh
herdr plugin action invoke send-pane --plugin dev.panemorph
herdr plugin action invoke bring-pane --plugin dev.panemorph
```

If the action works from the CLI, inspect your keybinding or host application's
shortcut settings. If the action fails, its log includes the error.

**The selector opens, but choosing an item fails**

Use paneMorph **0.1.2 or newer**. Earlier versions could attempt a move while the
selector still had the tab zoomed. On the current version, unzoom any source or
destination tab you zoomed yourself, then retry and inspect the action log.

**The bring selector is empty**

It only includes other tabs in the current workspace. Create another tab with
`Ctrl+T`, or use `Ctrl+E` to extract a pane first.

**Check the local connection**

From a paneMorph checkout inside Herdr, run:

```sh
python3 -m panemorph.doctor
```

This read-only check reports the Herdr version and topology counts. It requires
the `HERDR_SOCKET_PATH` environment variable supplied by Herdr.

## Development and testing

```sh
git clone https://github.com/Jenish-Shobhit/paneMorph.git
cd paneMorph
herdr plugin link "$PWD" --enabled
python3 -m unittest discover -s tests -v
```

Unit tests cover the socket client, move planning, selector lifecycle,
cancellation, and error handling.

For integration tests, start a disposable named server in a separate terminal:

```sh
herdr --session panemorph-test server
```

From the checkout in another terminal, link the plugin and use the **API socket
path printed by that test server**:

```sh
herdr --session panemorph-test plugin link "$PWD" --enabled
PYTHONPATH=. python3 tests/integration_herdr.py --socket /path/to/panemorph-test/herdr.sock
```

The integration script creates and removes its own workspace. It checks send to
new and existing tabs, bring individual panes and whole layouts, cancellation,
split geometry, and preservation of terminal identities. It only accepts sockets
in a directory named `panemorph-*`.

When finished, stop the test session:

```sh
herdr session stop panemorph-test
```

## Contributing and license

Bug reports and focused pull requests are welcome. For a bug report, include
your Herdr and paneMorph versions, the action you tried, and the relevant error
from `herdr plugin log list`. Remove private paths or terminal content before
posting logs.

Read [CONTRIBUTING.md](CONTRIBUTING.md), [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md),
and [SECURITY.md](SECURITY.md). paneMorph is released under the
[MIT license](LICENSE).
