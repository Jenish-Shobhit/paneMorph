# Contributing to paneMorph

Thanks for helping improve paneMorph.

## Development

1. Install herdr 0.9.0 or newer (0.9.1 recommended) and a Rust toolchain.
2. Fork and clone the repository, then run `cargo build --release`.
3. Link the checkout with `herdr plugin link "$PWD"`.
4. Before submitting a change, run `cargo test`, `cargo clippy --all-targets`
   and `cargo fmt --check`.

Test moves on a throwaway herdr server, never on terminals you cannot safely
rearrange. `PANEMORPH_LIVE=1 cargo test --release --test live_herdr --
--ignored` starts an isolated server of its own (see the README).

## Pull requests

- Keep changes focused and explain the user-visible workflow affected.
- Add tests for planning, failure handling, or window behaviour. Name tests
  after the edge-case rows they cover.
- Preserve live PTYs. A feature that restarts a terminal is not a paneMorph
  move and must not be presented as one.
- Update `CHANGELOG.md` for user-visible changes.

By contributing, you agree that your contribution is licensed under MIT.
