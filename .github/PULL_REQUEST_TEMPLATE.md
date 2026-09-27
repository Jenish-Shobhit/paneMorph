## What and why

<!-- What does this change, and which workflow does it affect? Link the issue it closes. -->

## How it was tested

<!-- Unit, rendering or pty tests added; live test on a throwaway herdr session, if relevant. -->

## Checklist

- [ ] `cargo fmt --all --check`, `cargo clippy --all-targets --locked -- -D warnings` and `cargo test --locked` pass
- [ ] New behaviour has a test, named after the edge-case row it covers where there is one
- [ ] Moves still use `pane.move` and never close or restart a terminal
- [ ] `CHANGELOG.md` has an entry under "Unreleased" for user-visible changes
- [ ] README and screenshots are updated if the windows changed (`cargo run --example screenshots`)
- [ ] No private paths, terminal output or real workspace names in code, tests or docs
