# Neko

Neko is a small Windows wallpaper manager with a quiet, dark gallery UI. It is written in Rust with [GPUI](https://gpui.rs/) and keeps the selected wallpaper folder and last-used browsing source between launches.

## What it does

- Browse images from one local folder with cached 480 px previews.
- Apply a local or downloaded wallpaper with the Windows desktop API.
- Rename files, move them to the Recycle Bin, reveal them in Explorer, and save online images into the local folder without overwriting an existing file.
- Search Wallhaven's SFW API with pagination.
- Browse the curated `bjarneo/wallpapers` catalog, cached for offline searches, with text and exact color filtering.
- Use a borderless, draggable floating window with keyboard-friendly controls.

The bjarneo live collection is intentionally omitted because it contains animated GIF/video assets and Windows' wallpaper API accepts a still image. Online originals are downloaded only when you apply or save them; previews are cached separately.

## Build and run

Install the stable Rust MSVC toolchain and Windows build tools, then run:

```powershell
cargo run --release
```

To make a redistributable folder and zip, run `powershell -ExecutionPolicy Bypass -File scripts/build-release.ps1`. The script uses `cargo build --release --locked` and writes only inside `dist/`.

Neko targets Windows. GPUI can resolve on other hosts during development, but wallpaper application is available only on Windows.

## Controls and data

`F5` refreshes the current collection, `Ctrl+O` chooses a folder, `Ctrl+F` focuses search, `Enter` submits a search, and `Esc` closes a preview or dialog. Click a thumbnail for its full preview; hover the preview and choose the monitor button for a quick apply.

Settings are stored in the platform configuration directory and provider/image caches in the platform cache directory. Set `NEKO_DATA_DIR` to a writable directory to keep both under that directory (useful for portable installs and tests). Neko validates image dimensions, encoded size, and decode memory before opening a file; see [docs/TESTING.md](docs/TESTING.md) for the exact limits and verification commands.

Wallhaven basic SFW search does not require an account or API key. Network requests use HTTPS, an allowlist of provider hosts, bounded responses, and explicit timeouts. No account, telemetry, favorites service, or cloud upload is included.

## Project map

`src/ui/` contains the GPUI window and native text input, `src/local/` owns safe filesystem operations and thumbnail caching, `src/sources/` owns both online providers and their cache, `src/wallpaper/` wraps Windows wallpaper application, and `src/config.rs` persists settings.

