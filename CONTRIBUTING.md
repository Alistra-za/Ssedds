# Contributing

Bug reports and pull requests are welcome. For a large change, open an issue first.

## Build and gates

Rust stable with the MSVC Build Tools (see [Installation](docs/wiki/Installation.md)); `rust-toolchain.toml` pins the rest. No ISPC install is needed.

```
cargo build --release -p sdc -p sdc-gui
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

CI runs the same on Windows. Tests that need owner data or a GPU are `#[ignore]`d or skip themselves.

## Conventions

- Licence: GPL-3.0-or-later. By contributing you agree your change is under it. Do not copy code from projects without a compatible licence.
- Design decisions are recorded in [.claude/DECISIONS.md](.claude/DECISIONS.md) as numbered notes. A change that alters behaviour adds or amends a note, and updates the wiki pages it touches.
- Output changes: a change that alters written bytes needs a line in [CHANGELOG.md](CHANGELOG.md) (and [docs/presets-changelog.md](docs/presets-changelog.md) for presets).
- No game assets in PRs: no `.dds`, `.nif`, `.bsa`, `.esp` or textures from Skyrim or from mods you do not own. Make small test fixtures yourself.
