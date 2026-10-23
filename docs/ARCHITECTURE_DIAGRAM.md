# RamShield Architecture Breakdown

```mermaid
flowchart TD
    %% Styles
    classDef ingress fill:#ff6b6b,color:#ffffff,stroke:#2d3436,stroke-width:2px
    classDef hook fill:#ffa502,color:#000000,stroke:#2d3436,stroke-width:2px
    classDef detection fill:#4ecdc4,color:#000000,stroke:#2d3436,stroke-width:2px
    classDef wal fill:#ffe66d,color:#000000,stroke:#2d3436,stroke-width:2px
    classDef crdt fill:#96ceb4,color:#000000,stroke:#2d3436,stroke-width:2px
    classDef peers fill:#ffe66d,color:#000000,stroke:#2d3436,stroke-width:2px

    %% Components
    subgraph Packet_Ingress [Packet Ingress]
        A[Network Traffic<br/>Incoming packets] -->|arrives at NIC| B[XDP Hook]
    end

    B[XDP Hook] -->|passes to kernel<br/>XDP program| C[EWMA/CUSUM Detection]
    
    C[EWMA/CUSUM Detection] -->|traffic anomaly detected| D[WAL Append]
    
    D[WAL Append] -->|persisted + synced| E[CRDT Mesh Sync]
    
    E[CRDT Mesh Sync] -->|ban propagated instantly| F[Peer Nodes]

    %% Styling
    class A ingress
    class B hook
    class C detection
    class D wal
    class E crdt
    class F peers

    %% Annotations
    click B "https://github.com/grep999/ramshield/blob/main/docs/ARCHITECTURE.md" "Architecture details"
    click C "https://github.com/grep999/ramshield/blob/main/docs/CONFIGURATION.md#thresholds" "Detection thresholds"
    click D "https://github.com/grep999/ramshield/blob/main/docs/ARCHITECTURE.md#wal-and-recovery" "WAL docs"
    click E "https://github.com/grep999/ramshield/blob/main/crates/ramshield-mesh" "Mesh docs"
    click F "https://github.com/grep999/ramshield/blob/main/docs/ARCHITECTURE.md" "Peer node overview"