fn main() {
    // Integration tests link tao/wry through Tauri's mock runtime, which
    // imports v6-only comctl32 entry points. The app binary gets a
    // suitable manifest from tauri-build, but test binaries do not, and
    // without one they abort with STATUS_ENTRYPOINT_NOT_FOUND at load
    // time. `rustc-link-arg-tests` scopes this to test targets only.
    #[cfg(windows)]
    {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("tests.manifest");
        if manifest.exists() {
            println!("cargo:rerun-if-changed={}", manifest.display());
            println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
            println!(
                "cargo:rustc-link-arg-tests=/MANIFESTINPUT:{}",
                manifest.display()
            );
        }
    }

    tauri_build::build()
}
