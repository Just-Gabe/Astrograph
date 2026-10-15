//! Icone e informacoes de versao do executavel (Windows).
fn main() {
    println!("cargo:rerun-if-changed=assets/astrograph.rc");
    println!("cargo:rerun-if-changed=assets/astrograph.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/astrograph.rc", embed_resource::NONE);
    }
}
