fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        link_clang_runtime();
    }
    tauri_build::build()
}

/// Link Apple clang's compiler-rt (`libclang_rt.osx.a`) into the macOS binary.
///
/// ONNX Runtime's CoreML code (statically linked via `ort`) uses `@available`
/// checks, which compile to calls to `__isPlatformVersionAtLeast`. rustc links
/// with `-nodefaultlibs`, so clang does not add compiler-rt itself. Rust's std
/// ships a weak copy of the helper, but with fat LTO (`[profile.release]
/// lto = true`) that copy is dropped before the linker sees ORT's objects, and
/// the release build fails with an undefined `___isPlatformVersionAtLeast`.
/// Linking compiler-rt explicitly provides the real definition; ld64 pulls in
/// only the archive members that resolve undefined symbols.
fn link_clang_runtime() {
    let resource_dir = std::process::Command::new("xcrun")
        .args(["clang", "--print-resource-dir"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string());

    let Some(resource_dir) = resource_dir else {
        println!("cargo:warning=could not locate clang resource dir via `xcrun clang --print-resource-dir`; not linking libclang_rt.osx.a");
        return;
    };

    let runtime = std::path::Path::new(&resource_dir).join("lib/darwin/libclang_rt.osx.a");
    if runtime.exists() {
        println!("cargo:rustc-link-arg={}", runtime.display());
    } else {
        println!(
            "cargo:warning={} not found; not linking clang's compiler-rt",
            runtime.display()
        );
    }
}
