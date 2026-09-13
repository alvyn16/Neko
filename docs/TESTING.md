# Testing Neko

Run the complete offline suite from the repository root:

```powershell
cargo fmt -- --check
cargo test -- --test-threads=4
cargo build --release --locked
```

The suite covers settings round trips and replacement, filename validation and collision handling, Recycle Bin and download naming helpers, corrupt/oversized image rejection, thumbnail invalidation and recovery, provider schema parsing, pagination, color/text filtering, URL allowlisting, bounded network responses, cache atomicity, Unicode text-input editing, and persistent wallpaper preparation. Tests use temporary directories and do not alter the desktop or delete user files.

The provider smoke test is opt-in because it uses the public internet and downloads one preview and original from each provider:

```powershell
cargo test online_provider_smoke -- --ignored --nocapture
```

Image input is capped at 100 MiB encoded, 80 megapixels, 16,384 pixels per side, and 512 MiB decoded allocation. Network indexes/downloads are bounded separately. A stale bjarneo catalog remains usable when refresh cannot reach the network.

For a manual UI pass, start `cargo run --release`, choose a folder containing a few JPG/PNG files, test Local/Explore tabs, source and color pills, preview/apply/save, rename, Explorer reveal, Recycle Bin delete, refresh, search submit, load more, and resize the window. The release script does not start the binary.

