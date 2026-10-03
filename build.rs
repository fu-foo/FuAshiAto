fn main() {
    println!("cargo:rerun-if-changed=res/fuashiato.rc");
    println!("cargo:rerun-if-changed=res/fuashiato.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("res/fuashiato.rc", embed_resource::NONE)
            .manifest_required()
            .expect("failed to compile resources");
    }
}
