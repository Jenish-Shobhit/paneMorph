# paneMorph

paneMorph is an open-source [Herdr](https://herdr.dev) plugin for moving live
terminal panes between tabs without restarting their processes.

It adds three layout actions and a direct two-key preset:

| Shortcut | Action |
| --- | --- |
| `Ctrl+T` | Create and focus a tab |
| `Ctrl+N` | Create a pane on the right |
| `Ctrl+]` / `Ctrl+[` | Next / previous tab |
| `Ctrl+L` / `Ctrl+H` | Next / previous pane |
| `Ctrl+\`` | Toggle the last active pane |
| `Ctrl+E` | Extract the focused pane to a background tab |
| `Ctrl+S` | Send the focused pane to a chosen tab |
| `Ctrl+F` | Fetch a tab or pane into the current tab on the right |

The complete behavior and topology examples are in [PRD.html](PRD.html).

## Why paneMorph preserves live processes

Herdr can move a pane while retaining its PTY, scrollback, working directory,
and running agent. For whole-tab imports, paneMorph exports the source split
tree, moves its live panes in a deterministic order, and reconstructs the same
directions and ratios inside the destination tab. It never uses
`layout.apply`, which would start replacement terminals.

## Requirements

- Herdr 0.9.0 or newer
- Python 3.10 or newer
- macOS or Linux

paneMorph has no third-party Python dependencies, telemetry, account, or
network service.

## Install from GitHub

```sh
herdr plugin install Jenish-Shobhit/paneMorph
```

Herdr intentionally does not let plugins silently take over keyboard
shortcuts. Review [config/panemorph-keys.toml](config/panemorph-keys.toml), then
merge the bindings you want into `~/.config/herdr/config.toml` and run:

```sh
herdr config check
herdr server reload-config
```

The direct preset replaces several familiar shell bindings, including
`Ctrl+S` flow control, `Ctrl+F` cursor movement, and `Ctrl+L` clear-screen.
Every action is rebindable.

## Local development

```sh
herdr plugin link "$PWD"
python3 -m unittest discover -s tests -v
python3 -m panemorph.doctor
```

The doctor command is read-only. It checks the current Herdr socket and prints
only version and topology counts.

For live integration testing, start a disposable named server with
`herdr --session panemorph-test server`. In another terminal, link this checkout
using `herdr --session panemorph-test plugin link "$PWD" --enabled`, then run
`PYTHONPATH=. python3 tests/integration_herdr.py --socket PATH`, using the API
socket path printed by that server. The script creates and removes its own test
workspace and refuses sockets outside a `panemorph-*` directory. Stop only the
test server afterward with `herdr session stop panemorph-test`.

## Safety model

- Complex moves are rejected while a source or destination tab is zoomed.
- Whole-tab moves are planned before the first mutation.
- If a later pane move fails, paneMorph attempts to return already-moved panes
  to the still-open source tab.
- Errors go to the Herdr plugin log and a best-effort local notification.
- Selectors close and restore the tab layout before applying the chosen move.
- The selector never includes another workspace; cross-workspace moves are out
  of scope for v1.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Security reports should follow
[SECURITY.md](SECURITY.md).

## License

MIT — see [LICENSE](LICENSE).
