//! Compila as gramaticas copiadas em `vendor/` (ver vendor/NOTICE.md).
fn main() {
    for name in ["luap", "luadoc"] {
        let dir = std::path::Path::new("vendor").join(name);
        println!("cargo:rerun-if-changed={}", dir.join("parser.c").display());
        cc::Build::new()
            .include(&dir)
            .file(dir.join("parser.c"))
            .warnings(false)
            .flag_if_supported("-std=c11")
            .compile(&format!("tree-sitter-{name}"));
    }
}
