# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What RABBIT is

RABBIT (REAPER Accessibility Bootstrap & Bundle Installation Tool) is a cross-platform installer/updater for the REAPER DAW plus accessibility packages (OSARA, SWS, ReaPack, ReaKontrol, etc.). It ships as a single self-contained executable with both a GUI wizard and a CLI, and targets screen-reader users.

## Workspace layout

Cargo workspace (Rust edition 2024), 5 crates under `crates/`:

- `rabbit` — entry-point binary (CLI by default; GUI behind the `gui` feature)
- `rabbit-cli` — clap-based CLI subcommands
- `rabbit-core` — main logic: arch detection, artifacts, install/rollback, upstream checks, i18n, REAPER config
- `rabbit-platform` — Windows/macOS abstractions (registry, elevation, signatures, paths, locales)
- `rabbit-ui-wxdragon` — keyboard-first GUI wizard (wxDragon)

Localizations live in `locales/{en-US,de-DE}/*.ftl` (Fluent). Workspace dependency versions are pinned in the root `Cargo.toml` under `[workspace.dependencies]` — add shared deps there, not per-crate.

## Commands

```bash
cargo fmt --all                          # format (CI enforces `cargo fmt --all --check`)
cargo clippy --workspace --all-targets   # lint (also runs in CI; warnings don't fail the build)
cargo test --workspace                   # run all tests
cargo build --release -p rabbit          # CLI-only binary
cargo build --release -p rabbit --features gui   # GUI binary
```

**GUI builds** (the `gui` feature) need `LIBCLANG_PATH` set (libclang for bindgen) and `ninja` on PATH — wxDragon's C++ wrapper builds with Ninja, not Make. CLI-only builds need neither.

## Conventions

- **Commits**: Conventional Commits with a scope, e.g. `fix(reapack): tolerate non-UTF-8 reapack.ini`, `feat(latest): ...`, `ci(macos): ...`.
- **Changelog**: every change adds an entry under `## [Unreleased]` in `CHANGELOG.md` (Keep a Changelog format; subsections Added/Changed/Deprecated/Removed/Fixed/Security). Releases are cut by the **prepare-release** GitHub Actions workflow, not by hand.
- **Accessibility is the point**: the app is built for NVDA/JAWS/Narrator/VoiceOver users. Keep UI changes keyboard-navigable and screen-reader-friendly.

## Gotchas

- Release profile is tuned for **download size** (`opt-level = "z"`, fat LTO, `panic = "abort"`) — release builds are slow and stripped; debug locally with `cargo build`/`cargo test`.
- The Windows release binary uses the `windows` subsystem (no console popup) but attaches the parent console at runtime so CLI output still redirects.
- `reapack.ini` / `reaper-kb.ini` may be UTF-8, UTF-16 (BOM), or ANSI — handle all encodings when touching that parsing code.
