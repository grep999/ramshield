use aya::Bpf;
use std::fs;

fn main() {
    let bpf_path = "../target/debug/build/ramshield-xdp/cac744c0b32c9ff3/out/ramshield-xdp";
    let bytes = fs::read(bpf_path).expect("failed to read BPF object");
    println!("Loaded {} bytes from {}", bytes.len(), bpf_path);
    
    match Bpf::load(&bytes) {
        Ok(bpf) => {
            println!("✓ BPF object loaded successfully");
            println!("Programs: {:?}", bpf.programs().map(|(k,_)| k).collect::<Vec<_>>());
            println!("Maps: {:?}", bpf.maps().map(|(k,_)| k).collect::<Vec<_>>());
        }
        Err(e) => {
            println!("✗ Failed to load: {}", e);
            std::process::exit(1);
        }
    }
}
