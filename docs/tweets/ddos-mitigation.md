DDoS mitigation architecture:
1. Edge filtering via eBPF XDP programs.
2. Rate limiting per IPC origin.
3. Anomaly detection via xgboost.
4. Fail-closed on engine crash.