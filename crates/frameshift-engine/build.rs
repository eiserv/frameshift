//! Embeds compiler identity and an explicit optional commit without inspecting Git.

use std::{env, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=RUSTC");
    println!("cargo:rerun-if-env-changed=FRAMESHIFT_BUILD_COMMIT");
    if let Some(commit) = env::var_os("FRAMESHIFT_BUILD_COMMIT") {
        let commit = commit.to_str().expect("build commit must be ASCII");
        assert!(
            commit.len() == 40
                && commit
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "build commit must be a complete lowercase Git SHA-1"
        );
        println!("cargo:rustc-env=FRAMESHIFT_BUILD_COMMIT={commit}");
    }
    let compiler = env::var_os("RUSTC").expect("Cargo must identify the Rust compiler");
    let output = Command::new(compiler)
        .arg("--version")
        .output()
        .expect("the build must be able to identify its Rust compiler");
    assert!(output.status.success(), "compiler identity command failed");
    let identity = std::str::from_utf8(&output.stdout)
        .expect("compiler identity must be UTF-8")
        .trim();
    assert!(
        identity.starts_with("rustc ")
            && identity.bytes().all(|byte| (b' '..=b'~').contains(&byte)),
        "compiler identity must be a single printable ASCII line"
    );
    println!("cargo:rustc-env=FRAMESHIFT_RUSTC_VERSION={identity}");
}
