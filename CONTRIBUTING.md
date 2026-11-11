# Contributing to Remap75

Thanks for helping. Issues and pull requests are welcome.

## How changes get in

- Fork the repo, branch from `main`, and open a pull request. Only the maintainer can merge.
- `main` is protected: every change goes through a PR, history stays linear, and PRs are
  squash-merged. Keep each PR to one change.
- Workflows from first-time contributors need the maintainer's approval before they run.

## Before you open a PR

- `cargo test -p aula75-data` for protocol, layout, macro, service and replay tests.
- `bun exact.mjs test web` for the UI tests against the simulated keyboard.
- If you touch the protocol, add a test against the simulated keyboard (`data/src/sim.rs`).
- Site changes: `cd site && bun install && bun run build`.

See [Developing](README.md#developing) for setup. The Rust crates depend on a local checkout
of [Exact](https://github.com/rosman21/aula75-remapper#how-exact-is-used) by path, so the Rust
parts can't build in plain CI yet. Please run the tests above yourself and say so in the PR.

## AI-assisted contributions

This project is itself vibe coded, so AI-assisted PRs are fine. You're still responsible for
what you submit: read it, run it, and say in the PR if an AI wrote it. For anything that writes
to the keyboard, test on real hardware or explain how you tested it. A bad write can need a
factory reset (Fn+Esc for about 5 seconds).

## Other keyboards and firmware

Reports of what works on other AULA models or firmware versions are very useful. Use the
"Keyboard compatibility" issue template and include the output of
`cargo run -p aula75-data --example probe` (read-only).

## Licensing

By contributing you agree your work is released under the [MIT License](LICENSE).

## Conduct

Be kind and assume good faith. See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
