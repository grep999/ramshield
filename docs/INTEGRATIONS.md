# Integration notes

- nftables: coexist; keep ownership of chains explicit. XDP runs earlier.
- nginx / Caddy / Traefik: RamShield handles L3/L4 only; proxy keeps its own limits.
- Game servers: protect the listen host; no protocol rewriting.
- Upstream: FlowSpec/RTBH belongs to the ISP, not this binary.

See `docs/DEPLOYMENTS.md`.
