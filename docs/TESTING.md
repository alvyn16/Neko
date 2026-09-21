# Testing Neko

Run the complete offline suite from the repository root:

```powershell
cargo fmt -- --check
cargo test -- --test-threads=4
cargo build --release --locked
```

The suite covers settings round trips and replacement, including unknown-provider fallback; filename validation and collision handling; Recycle Bin and download naming helpers; corrupt/oversized image rejection; thumbnail invalidation and recovery; provider schema parsing; pagination; color, text, and Frenzy category filtering; URL construction and host allowlisting; bounded network responses; cache atomicity; Unicode text-input editing; and persistent wallpaper preparation. Frenzy fixtures also cover extension, folder, tile-texture, and truncated-tree filtering, plus ETag reuse, stale-cache fallback for rate limits, missing repositories, and network failures, and protection of a valid cache when a refresh is malformed. Tests use temporary directories and do not alter the desktop or delete user files.

The provider smoke test is opt-in because it uses the public internet and downloads one preview and original from each provider:

```powershell
cargo test online_provider_smoke -- --ignored --nocapture
```

Image input is capped at 100 MiB encoded, 80 megapixels, 16,384 pixels per side, and 512 MiB decoded allocation. Network indexes/downloads are bounded separately. Stale bjarneo and Frenzy catalogs remain usable when refresh cannot reach the network; Frenzy also uses its cached catalog when GitHub rate-limits requests or the repository is unavailable.

For a manual UI pass, start `cargo run --release`, choose a folder containing a few JPG/PNG files, and test Local/Explore tabs, all three source pills, bjarneo color pills, Frenzy category pills, preview/apply/save, rename, Explorer reveal, Recycle Bin delete, refresh, search submit, load more, drag-and-drop importing, and resizing the window. For Frenzy, verify category and filename search, pagination, exact preview attribution, and cached results after disconnecting the network. Open Settings and test all five fit modes, each connected display target, Rotate now, changing the automatic rotation interval, and Check now. The release script does not start the binary.

Build the installer with `powershell -ExecutionPolicy Bypass -File scripts/build-installer.ps1`. Install it for the current user, launch it from the Start Menu shortcut, pin that installed shortcut to the taskbar, and verify uninstall from Windows Settings.
