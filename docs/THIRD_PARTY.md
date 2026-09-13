# Third-party notices

Neko uses the crates listed in `Cargo.lock`, each under its own license and notice terms. The most visible upstream components are:

- GPUI, from the Zed project, under Apache-2.0. Neko's native text input adapts the structure of GPUI's public `examples/input.rs`; the implementation remains in `src/ui/input.rs` and this attribution is retained for that adaptation.
- `reqwest`, `serde`, `image`, `directories`, `rfd`, `trash`, `sha2`, `toml`, and their transitive dependencies under their respective package licenses.
- Wallhaven and the bjarneo/wallpapers gallery are external services. Neko displays provider metadata and downloads only on the user's request; consult each service's current terms for the images you use.

