fn main() {
    println!("cargo:rerun-if-changed=neko.rc");
    println!("cargo:rerun-if-changed=assets/neko-icon.ico");

    #[cfg(windows)]
    embed_resource::compile("neko.rc", embed_resource::NONE)
        .manifest_optional()
        .unwrap();
}
