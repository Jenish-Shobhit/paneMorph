# Contributing to paneMorph

Thanks for helping improve paneMorph. Bug reports, focused pull requests and
well-argued ideas are all welcome. Everyone taking part follows the
[code of conduct](CODE_OF_CONDUCT.md). Report security problems privately, as
described in [SECURITY.md](SECURITY.md).

## Development setup

You need:

- Rust. `rust-toolchain.toml` selects stable with rustfmt and clippy, and
  rustup installs it on first use. The minimum supported version is 1.89
  (`rust-version` in `Cargo.toml`), and CI checks it.
- macOS or Linux.
- herdr 0.9.0 or newer (0.9.1 recommended), only for trying changes in a real
  session and for the live test.

```sh
git clone https://github.com/Jenish-Shobhit/paneMorph.git
cd paneMorph
cargo build --release
cargo run -- preview send     # or fetch: a window over a simulated session
```

`preview` needs no herdr, so most window work can happen there.

## Checks

CI runs these on Ubuntu and macOS. Run them before you open a pull request:

```sh
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked
cargo deny check              # optional locally: cargo install cargo-deny
```

CI also builds and tests on Rust 1.89, and regenerates the simulated
screenshots to check that `docs/assets/` is current.

## Tests

- **Unit tests** (`src/**`) run the planners, naming, undo journal and
  executor against an in-memory herdr ([src/sim.rs](src/sim.rs)) that mirrors
  herdr 0.9's behaviour, and render the windows into ratatui's `TestBackend`.
- **Pseudo-terminal tests** ([tests/pty_windows.rs](tests/pty_windows.rs))
  drive the real binary's `preview` windows with real key bytes and check
  rendering, filtering, mouse, resize and ⎋ timing.
- **The live test** ([tests/live_herdr.rs](tests/live_herdr.rs)) presses every
  key through a real herdr client. It is `#[ignore]`d and opt-in.

Name a test after the edge-case row it covers, for example
`edge_1_6_last_pane_closes_tab`. [docs/verification.md](docs/verification.md)
lists every row and the tests that cover it. A change in behaviour needs a
test that shows it.

### Live tests on a throwaway herdr server

Never test moves on a herdr session you care about. The live test starts its
own server with a private `XDG_CONFIG_HOME`, `XDG_STATE_HOME`,
`XDG_DATA_HOME` and `XDG_CACHE_HOME`, registers this checkout only in that
private plugin registry, removes inherited `HERDR_*` variables, and refuses
any socket outside its private directory:

```sh
cargo build --release
PANEMORPH_LIVE=1 cargo test --release --test live_herdr -- --ignored --nocapture
HERDR_BIN=/path/to/herdr PANEMORPH_LIVE=1 cargo test --release --test live_herdr -- --ignored
```

To try a change by hand, use the same isolation: point every XDG variable
and `HOME` at a temporary directory, and start a named session there. Do not
run `herdr plugin link`, `server stop` or `server reload-config` against your
own session while testing.

## Screenshots

The README screenshots are generated, never edited by hand:

```sh
scripts/screenshots.sh            # all screenshots; the hero needs herdr
scripts/screenshots.sh --no-live  # only the windows over the demo session
```

`cargo run --example screenshots` renders Send, Fetch and the notice popup
with the plugin's own drawing code over
[examples/screenshots/demo-session.json](examples/screenshots/demo-session.json).
`--live` captures the hero from a real herdr client in a throwaway session
with a private home directory, so no personal prompt, path or name can
appear. Commit the regenerated files in `docs/assets/` with the change that
caused them.

## Project layout

| Path | What it holds |
| --- | --- |
| `src/actions.rs` | Command-line entry: plugin actions, popup entrypoints, `doctor`, `preview` |
| `src/api.rs`, `src/model.rs` | herdr socket client and the typed snapshot |
| `src/plan.rs`, `src/topology.rs` | Pure move decisions and split-tree replay |
| `src/exec.rs`, `src/journal.rs` | Carrying out moves, rollback and undo |
| `src/names.rs` | How panes, tabs and spaces are named |
| `src/state.rs` | Invocation context, the key queue lock, logging, herdr config reads |
| `src/sim.rs` | The in-memory herdr used by tests and `preview` |
| `src/ui/` | The windows: state and keys, rows, drawing, terminal loop, worker, notice |
| `examples/screenshots/` | The screenshot generator and its demo session |

## Pull requests

- Keep each pull request to one change, and explain the workflow it affects.
- Moves must stay live: use `pane.move` and friends, never close, recreate
  or restart a terminal.
- Add an entry under "Unreleased" in [CHANGELOG.md](CHANGELOG.md) for any
  user-visible change.
- Keep private data out of code, tests, docs and screenshots: no real
  workspace names, home paths or terminal output.

### Commit style

Commits follow [Conventional Commits](https://www.conventionalcommits.org/):
`type: summary` in the imperative, at most 72 characters, with a body that
says why. Types in use are `feat`, `fix`, `docs`, `test`, `refactor`,
`build`, `ci` and `chore`; mark breaking changes with `!`, as in
`refactor!: remove the Python package`.

## Releasing

Maintainers release from `main`:

1. Set the version in `Cargo.toml` and `herdr-plugin.toml`, and run
   `cargo build` to update `Cargo.lock`.
2. Move the "Unreleased" entries in `CHANGELOG.md` under the new version and
   date, and update the links at the bottom.
3. Commit, wait for CI to pass, then tag and push: `git tag vX.Y.Z && git
   push origin vX.Y.Z`.

The release workflow checks that the tag matches both manifests and the
changelog, builds the macOS and Linux archives, and publishes the GitHub
release with that version's changelog section as its notes.

## License

By contributing, you agree that your contributions are licensed under the
[MIT license](LICENSE).
