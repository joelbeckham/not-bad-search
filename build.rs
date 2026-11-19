fn main() {
    println!("cargo:rerun-if-changed=app-icon.rc");
    println!("cargo:rerun-if-changed=not-bad-search-icon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile_for("app-icon.rc", ["not-bad-search"], embed_resource::NONE)
            .manifest_required()
            .expect("failed to embed the Not Bad Search icon");
    }
}
