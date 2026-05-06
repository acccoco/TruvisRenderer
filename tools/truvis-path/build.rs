fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let root = std::path::Path::new(&manifest)
        .parent()
        .unwrap() // tools/
        .parent()
        .unwrap(); // workspace 根目录
    println!("cargo:rustc-env=TRUVIS_WORKSPACE_ROOT={}", root.display());
    println!("cargo:rerun-if-changed=../../map.toml");
}
