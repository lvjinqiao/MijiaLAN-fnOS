use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=resources/windows/app.manifest");
    println!("cargo:rerun-if-changed=resources/windows/app.rc");
    println!("cargo:rerun-if-changed=resources/windows/app.ico");
    println!("cargo:rerun-if-changed=resources/windows/icon.rc");
    println!("cargo:rerun-if-env-changed=MIJIALAN_WINDRES");
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let host = env::var("HOST").unwrap();
    if target_os != "windows" {
        return;
    }
    if host.contains("-windows-") {
        // GPUI supplies the native Windows manifest; this resource only adds
        // our application icon, avoiding duplicate manifest IDs at link time.
        embed_resource::compile(
            "resources/windows/icon.rc",
            embed_resource::ParamsIncludeDirs(["resources/windows"]),
        )
        .manifest_required()
        .expect("Unable to embed the Windows application icon");
        return;
    }
    assert_eq!(
        env::var("CARGO_CFG_TARGET_ENV").unwrap(),
        "gnu",
        "Cross-compiling the Windows desktop currently requires the GNU target"
    );
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("windows-resources.o");
    let compiler =
        env::var_os("MIJIALAN_WINDRES").unwrap_or_else(|| "x86_64-w64-mingw32-windres".into());
    let status = Command::new(compiler)
        .current_dir("resources/windows")
        .args(["--input", "app.rc", "--output-format=coff", "--output"])
        .arg(&output)
        .status()
        .expect("Unable to run MinGW windres for the Windows application manifest");
    assert!(status.success(), "Windows resource compilation failed");
    println!(
        "cargo:rustc-link-arg-bin=mijialan-rust={}",
        output.display()
    );
}
