//! Records compiler identity without inspecting Git or the source environment.

use std::{env, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=RUSTC");
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
