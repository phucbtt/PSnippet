# Contributing

Thanks for helping! Issues and PRs are welcome, in English or Vietnamese.

## Ground rules (product safety)
- Never add code that sends/submits a message (no Enter/Send injection), reads chat content, or logs typed text.
- Keep all `unsafe` and Win32 calls inside `crates/platform-windows`; other crates stay pure and unit-testable.
- Hook callbacks must stay tiny and fail open (no I/O, no waiting, no panics).
- Do not mark Zalo behaviour as verified without evidence from a real device (update `docs/compatibility-matrix.md`).

## Workflow
1. Fork, create a branch, make the change.
2. Run: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --locked`.
3. Add or update tests. Add a short change record in `docs/specs/<YYYY-MM-DD>-<slug>.md`
   (Context/Why, Decision, Design, API/Schema changes, How to test).
4. Open a PR describing what you tested and on which Windows version.

Manual smoke test: `tests/manual/smoke-notepad.ps1` (it installs a keyboard hook and sends keystrokes; run it yourself, not in CI).
