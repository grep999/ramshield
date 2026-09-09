# RamShield Module Documentation Standard

## Audience

Engineer evaluating RamShield for production deployment. Has read the root README.
Knows Rust, DDoS mitigation concepts, and Linux networking. Has not read the source.

## Structure per module (fixed, all 10)

1. **One-paragraph pitch** — What this crate does in the system. No fluff.
2. **Why it exists** — The specific problem it solves. Not "DDoS is bad."
3. **Architecture** — ASCII diagram of data flow IN and OUT of this crate.
4. **Public API** — Every pub struct, pub fn, pub const. Real signatures.
5. **Internal machinery** — Algorithms that matter. Skip the obvious.
6. **Integration points** — What sends data here, what reads from here.
7. **Benchmarks** — Where measured, numbers from `cargo bench --bench hot_paths`.
8. **Testing** — Test count, what's covered, what's not.

## Rules

- Every sentence earns its place. Cut anything that restates the previous sentence.
- No "RamShield is a..." openers. State what the crate does.
- No "This module handles..." openers. State the function.
- Code blocks show real type signatures, not pseudocode.
- ASCII diagrams use only `→` and `│` for flow, no box-drawing.
- Each README is self-contained. A reader should not need another README to understand this one.
- "What's unique" gets called out explicitly. "Essential but standard" gets one sentence.
- Zero marketing language. Zero "powerful", "robust", "cutting-edge".
- Benchmarks reference the exact `cargo bench` command.
