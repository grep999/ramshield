//! Compile the XDP object into OUT_DIR/ramshield-xdp for include_bytes_aligned!.
//!
//! Path B (preferred): aya-ebpf + bpf-linker → bpfel-unknown-none.
//! Fallback: clang -target bpf on ramshield-xdp-bpf/src/main.c.
//! Last resort: empty ELF-looking stub so host `cargo check` still compiles
//! userspace without a BPF toolchain. Runtime load() fails on the stub.
//!
//! ponytail: drop the stub when bpf-linker matches host LLVM (needs LLVM 21 APIs;
//! Ubuntu noble ships LLVM 18). Upgrade: cargo install bpf-linker against matching llvm-sys.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=ramshield-xdp-bpf/src/main.rs");
    println!("cargo:rerun-if-changed=ramshield-xdp-bpf/src/main.c");
    println!("cargo:rerun-if-changed=ramshield-xdp-bpf/Cargo.toml");
    println!("cargo:rerun-if-changed=bpf/main.rs");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let dest = out_dir.join("ramshield-xdp");

    if try_aya_build(&dest) {
        return;
    }
    if try_clang_c(&dest) {
        return;
    }
    write_stub(&dest);
    println!(
        "cargo:warning=XDP BPF object stubbed — bpf-linker/clang path failed. load() will fail at runtime."
    );
}

fn try_aya_build(dest: &Path) -> bool {
    // aya-build 0.2 expects a package name; skip if bpf-linker missing.
    if Command::new("bpf-linker")
        .arg("--version")
        .output()
        .is_err()
    {
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
            return fs::copy(&c, dest).is_ok();
        }
    }
    false
}

fn try_clang_c(dest: &Path) -> bool {
    let src = Path::new("ramshield-xdp-bpf/src/main.c");
    if !src.exists() {
        return false;
    }
    let tmp = dest.with_extension("o");
    let status = Command::new("clang")
        .args([
            "-O2",
            "-g",
            "-target",
            "bpf",
            "-c",
            "-I/usr/include",
            "-I/usr/include/x86_64-linux-gnu",
            "-D__TARGET_ARCH_x86",
            "-Wno-unused-command-line-argument",
            src.to_str().unwrap(),
            "-o",
            tmp.to_str().unwrap(),
        ])
        .status();
    match status {
        Ok(s) if s.success() && tmp.exists() => {
            fs::rename(&tmp, dest).is_ok() || fs::copy(&tmp, dest).is_ok()
        }
        _ => false,
    }
}

fn write_stub(dest: &Path) {
    // Minimal ELF64 LE header so aya::Bpf::load fails with a parse error, not ENOENT.
    let mut elf = vec![
        0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0xf7, 0, 1, 0, 0, 0,
    ];
    elf.resize(64, 0);
    let _ = fs::write(dest, elf);
}
