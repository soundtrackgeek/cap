fn main() {
    println!("cargo:rerun-if-changed=Cargo.toml");
    let manifest = std::fs::read_to_string("Cargo.toml").expect("package manifest");
    let dependency = manifest
        .lines()
        .find(|line| line.starts_with("capsule-core ="))
        .expect("pinned shared Capsule core dependency");
    let revision = dependency
        .split("rev = \"")
        .nth(1)
        .and_then(|text| text.split('"').next())
        .expect("Capsule core revision");
    assert!(revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit()));
    println!("cargo:rustc-env=CAP_CORE_REVISION={revision}");
    println!(
        "cargo:rustc-env=CAP_LONG_VERSION={} (Capsule core {}; desktop baseline 0.37.0)",
        env!("CARGO_PKG_VERSION"),
        &revision[..12]
    );
}
