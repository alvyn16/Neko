# Contributing to Neko

Thanks for helping improve Neko. Keep changes focused, describe the user-visible effect, and avoid unrelated formatting or refactors.

## Before opening an issue

Search existing issues first. For bugs, include the Neko version, Windows version, clear reproduction steps, expected and actual behavior, and relevant logs or screenshots. Do not include private paths, credentials, or personal images.

## Development

Neko targets Windows and uses the stable Rust MSVC toolchain. From the repository root, run:

```powershell
cargo fmt -- --check
cargo test -- --test-threads=4
cargo build --release --locked
```

See [docs/TESTING.md](docs/TESTING.md) for the opt-in provider smoke test and manual UI checks.

## Pull requests

Explain what changed and why, link related issues where applicable, and include tests or manual verification for behavior changes. Keep documentation in sync with visible behavior. Do not submit wallpapers or other third-party image assets for inclusion; Neko fetches online images on demand.
