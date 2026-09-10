//! Compile the XDP BPF object via aya-ebpf + bpf-linker.
//!
//! Produces a BPF ELF at OUT_DIR/ramshield-xdp for include_bytes_aligned!.
//! All loading, attaching, and map management lives in
//! ramshield-enforcement::xdp::AyaXdpApplier.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=ramshield-xdp-bpf/src/main.rs");
    println!("cargo:rerun-if-changed=ramshield-xdp-bpf/Cargo.toml");
    println!("cargo:rerun-if-changed=bpf/main.rs");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let dest = out_dir.join("ramshield-xdp");

    if try_aya_build(&dest) {
        return;
    }

    panic!(
        "XDP BPF build failed: bpf-linker not found or cargo build failed.\n\
         Install bpf-linker: https://github.com/aya-rs/bpf-linker/releases\n\
         Then: PATH=\"$HOME/.local/bin:$PATH\" cargo build"
    );
}

fn try_aya_build(dest: &Path) -> bool {
    if Command::new("bpf-linker")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!(
            "cargo:warning=bpf-linker not found — install from https://github.com/aya-rs/bpf-linker/releases"
        );
        return false;
    }
    let manifest = Path::new("ramshield-xdp-bpf/Cargo.toml");
    if !manifest.exists() {
        return false;
    }
    let status = Command::new("cargo")
        .current_dir("ramshield-xdp-bpf")
        .args([
            "build",
            "--release",
            "--target=bpfel-unknown-none",
            "-Z",
            "build-std=core",
        ])
        .status();
    let Ok(s) = status else {
        return false;
    };
    if !s.success() {
        return false;
    }
    let candidates = [
        PathBuf::from("ramshield-xdp-bpf/target/bpfel-unknown-none/release/ramshield-xdp-bpf"),
        PathBuf::from("ramshield-xdp-bpf/target/bpfel-unknown-none/release/ramshield-xdp"),
    ];
    for c in candidates {
        if c.exists() {
            return std::fs::copy(&c, dest).is_ok();
        }
    }
    false
}
