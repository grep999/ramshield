# Research Flow

Automated research findings feeding the pipeline. Populated by the research agent.
Each entry links to a backlog item or roadmap task it informs.

- task_id: roadmap/Q1-eBPF, summary: eBPF/XDP Integration, links: https://github.com/aya-rs/aya, suggested_implementation: Use aya-rs for safe kernel-level packet filtering.
- task_id: roadmap/Q1-eBPF, summary: XDP drop path + aya perf-array → Ramshield batch channel, links: https://aya-rs.github.io/book/, https://docs.rs/aya/latest/aya/, https://github.com/libbpf/libbpf, suggested_implementation: Attach XDP drop program via aya::programs::xdp; forward per-CPU counters through BPF_MAP_TYPE_PERF_EVENT_ARRAY into dedicated tokio task and route into the existing 2M-event batch channel.
- task_id: roadmap/Q2-tflite, summary: Rust ML Inference, links: https://github.com/rust-ml, suggested_implementation: Explore rust-ml/discussion to identify mature ML inference crates for XGBoost as tflite-rust support is limited/stale.
- task_id: roadmap/Q1-linfa, summary: Classical ML Algorithms, links: https://github.com/rust-ml/linfa, https://docs.rs/linfa, suggested_implementation: Integrate linfa crate for classical supervised/unsupervised algorithms as robust Rust alternative to Python scikit-learn.
