# RamShield Development Roadmap

## Proposition: Edge-Native Sovereign Defense
RamShield: On-prem, edge-native, sovereign DDoS detection.
- Sovereignty: No data egress, private residency.
- Efficiency: 30K events/s per node, 4.8MB RAM footprint (benchmark).
- Density: Deployable on edge SoCs/routers.
- Advantage: Mitigate 90% malicious traffic at perimeter, reducing egress costs.

## Industry Landscape
| Platform | Peak Capacity | Architecture | Cost Model |
| :--- | :--- | :--- | :--- |
| Cloudflare | 3.2 Pbps | Global Edge | Usage |
| AWS Shield | 2.1 Pbps | 21 Regions | Tiered |
| **RamShield** | **3.8 Tbps (sim)** | **Edge-native** | **Fixed/Opex** |

## Enterprise Development Roadmap
| Phase | Focus | Tech/Integration | Goal |
| :--- | :--- | :--- | :--- |
| I | Clustering | Raft, Gossip | Distributed state |
| II | Kernel-Speed | eBPF, XDP | Kernel-level L3/L4 filtering |
| III | Intelligence | Isolation Forest | Zero-day anomaly detection |
| IV | Scale Ops | OPA, Prometheus | Enterprise config management |

## Integration Targets (Inspiration)
- **Kernel-level:** `xdp-firewall` (eBPF/XDP) for line-rate packet drops.
- **Analytics:** `apache/datafusion` as the query engine for traffic flows.
- **Consistency:** `Raft` / `Tokio-Gossip` for cluster synchronization.
- **Anomaly Detection:** `Extended Isolation Forest` for ML-driven pattern discovery.
