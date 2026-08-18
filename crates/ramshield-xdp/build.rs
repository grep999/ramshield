// ponytail: BPF compilation deferred — needs bpfel target + clang.
// Emit a no-op build script so workspace `cargo build` succeeds without
// a BPF toolchain. Upgrade: use aya-build when target is available.
fn main() {
    println!("cargo:rerun-if-changed=ramshield-xdp-bpf/src/main.c");
}
