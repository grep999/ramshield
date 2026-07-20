# Write dev.to article: "Zero-trust IPC in 50 lines"

Focus on:
- Small code footprint
- Performance (shm vs unix sockets)
- Security (uid/gid checks)

Code snippet:
```rust
// ponytail: basic IPC. upgrade to ring-based shm later.
fn main() {
    println!("Zero-trust IPC incoming");
}
```
