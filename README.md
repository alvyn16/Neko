<p align="center">
  <img src="assets/neko-icon.svg" width="112" alt="Neko logo">
</p>

<h1 align="center">Neko</h1>

<p align="center">A quiet, native wallpaper manager for Windows.</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/github/license/alvyn16/Neko?style=flat-square" alt="MIT License"></a>
  <a href="https://github.com/alvyn16/Neko/releases"><img src="https://img.shields.io/github/v/release/alvyn16/Neko?display_name=tag&amp;sort=semver&amp;style=flat-square" alt="Latest release"></a>
  <a href="https://github.com/alvyn16/Neko/actions/workflows/release.yml"><img src="https://img.shields.io/github/actions/workflow/status/alvyn16/Neko/release.yml?branch=main&amp;style=flat-square&amp;label=build" alt="Build status"></a>
  <img src="https://img.shields.io/badge/platform-Windows-0078D4?style=flat-square&amp;logo=windows&amp;logoColor=white" alt="Windows">
</p>

## Preview

![Neko wallpaper gallery on Windows](docs/images/neko-preview.png)

## Features

- Browse a local wallpaper folder with cached previews, then apply images to all displays or a selected monitor.
- Search Wallhaven's SFW API and browse the curated `bjarneo/wallpapers` and `FrenzyExists/wallpapers` catalogs.
- Save online images to the local folder, import images by drag and drop, and manage files from the gallery.
- Choose Fill, Fit, Stretch, Center, or Tile placement and rotate local wallpapers while Neko is running.
- Check GitHub Releases and install verified updates.

## Install

### Installer

Download [Neko-Setup-x64.exe](https://github.com/alvyn16/Neko/releases/latest/download/Neko-Setup-x64.exe) from the latest release and run it. The per-user installer adds a Start Menu shortcut, can create a desktop shortcut, and includes an uninstaller.

### Build from source

Install the stable Rust MSVC toolchain and Windows build tools, then run:

```powershell
cargo run --release
```

To create distributable artifacts, use:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/build-release.ps1
```

For the Windows installer, install [Inno Setup 6](https://jrsoftware.org/isinfo.php) and run:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/build-installer.ps1
```

## Quickstart

1. Start Neko and choose a wallpaper folder with `Ctrl+O`.
2. Select an image to open its preview, then choose a monitor to apply it.
3. Use the gear button for placement, display targeting, rotation, and update settings.
4. Switch sources to browse local images, Wallhaven, bjarneo, or Frenzy; use `Ctrl+F` to search and `Enter` to submit. Frenzy can also be narrowed by its folder categories.

`F5` refreshes the current collection. `Esc` closes a preview or dialog.

<details>
<summary>Security &amp; privacy</summary>

Neko validates image input before opening it: 100 MiB encoded, 80 megapixels, 16,384 pixels per side, and 512 MiB decoded allocation. Network requests use HTTPS, provider-host allowlisting, bounded responses, and explicit timeouts. Settings and provider/image caches remain on the device; set `NEKO_DATA_DIR` to a writable directory to keep both in one location. Neko has no account, telemetry, favorites service, or cloud upload.

See [testing and limits](docs/TESTING.md) for verification commands and further detail.
</details>

## Sources & credits

- [Wallhaven](https://wallhaven.cc/) provides the SFW search source.
- [bjarneo/wallpapers](https://github.com/bjarneo/wallpapers) provides the curated catalog source.
- [FrenzyExists/wallpapers](https://github.com/FrenzyExists/wallpapers) provides a folder-based catalog. Its repository license covers the repository code and does not grant rights to the wallpaper images.

Neko does not bundle, mirror, or redistribute these wallpapers. It fetches catalog data and visible previews on demand, and fetches originals when you apply or save them. Use of the images is subject to their respective terms, copyright, and licenses.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development, testing, and issue guidelines. Third-party notices are in [docs/THIRD_PARTY.md](docs/THIRD_PARTY.md).
