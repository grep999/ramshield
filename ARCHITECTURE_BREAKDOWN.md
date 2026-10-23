# Architecture Breakdown

## System Flow

```mermaid
flowchart TD
    %% Component styles
    classDef ingress fill:#ff6b6b,color:#ffffff,stroke:#333,stroke-width:2px
    classDef xdpHook fill:#ffa502,color:#000,stroke:#333,stroke-width:2px
    classDef detection fill:#4ecdc4,color:#000,stroke:#333,stroke-width:2px
    classDef wal fill:#ffe66d,color:#000,stroke:#333,stroke-width:2px
    classDef crdt fill:#95e1d3,color:#000,stroke:#333,stroke-width:2px
    classDef peers fill:#f7b267,color:#000,stroke:#333,stroke-width:2px

    %% Architecture diagram
    subgraph Packet_Ingress [Packet Ingress]
        A[Network Traffic<br/>-> enters NIC] -->|arrives| B[XDP Hook]
    end

    B[XDP Hook] -->|processes via eBPF| C[EWMA/CUSUM Detection]
    C[EWMA/CUSUM Detection] -->|anomalies detected| D[WAL Append]
    D[WAL Append] -->|persists and syncs| E[CRDT Mesh Sync]
    E[CRDT Mesh Sync] -->|propagation to| F[Peer Nodes]

    %% Styling
    class A,B ingress,xdpHook
    class C detection
    class D wal
    class E crdt
    class F peers

    %% Notes
    click B "javascript:void(0)" "XDP: kernel-level packet filtering"
    click C "javascript:void(0)" "EWMA/CUSUM: streaming anomaly detection"
    click D "javascript:void(0)" "WAL: write-ahead log for durability"
    click E "javascript:void(0)" "CRDT: conflict-free replicated data types"
    click F "javascript:void(0)" "Peer Nodes: mesh network members"
```

## Component Details

| Component | Description | Technology |
|-----------|-------------|------------|
| **Packet Ingress** | Network traffic entering the system | NIC, drivers |
| **XDP Hook** | Kernel eBPF filter for packet inspection | eBPF/XDP |
| **EWMA/CUSUM Detection** | Real-time anomaly detection algorithms | Rate limiting, statistical analysis |
| **WAL Append** | Write-ahead log for persistence | File system |
| **CRDT Mesh Sync** | Conflict-free replication for distributed nodes | CRDT algorithms |
| **Peer Nodes** | Mesh network members | TCP/IP, networking |

## Key Features Demonstrated

1. **Zero-Trust Architecture**: Each component operates independently
2. **Real-Time Detection**: EWMA/CUSUM algorithms for streaming analysis
3. **Fault Tolerance**: Mesh network with CRDT synchronization
4. **Kernel-Bypass**: XDP for high-performance packet filtering
5. **Persistent Storage**: WAL for recovery and audit trails

## Integration Points

- **Detection Layer**: Monitors all incoming traffic patterns
- **Enforcement Layer**: Applies blocks based on detection results
- **Storage Layer**: Maintains state and history
- **Network Layer**: Facilitates peer-to-peer communication