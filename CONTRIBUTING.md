# Contributing to paneMorph

Thanks for helping improve paneMorph.

## Development

1. Install Herdr 0.9.0 or newer and Python 3.10 or newer.
2. Fork and clone the repository.
3. Link the checkout with `herdr plugin link "$PWD"`.
4. Run `python3 -m unittest discover -s tests -v` before submitting a change.

Use a named test session for destructive topology experiments. Never test pane
moves against terminals you cannot safely rearrange.

## Pull requests

- Keep changes focused and explain the user-visible workflow affected.
- Add tests for topology planning, failure handling, or selector behavior.
- Preserve live PTYs. A feature that restarts a terminal is not a paneMorph
  move and must not be presented as one.
- Update `CHANGELOG.md` for user-visible changes.

By contributing, you agree that your contribution is licensed under MIT.

