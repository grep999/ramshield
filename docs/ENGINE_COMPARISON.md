# Engine comparison

Scope: open-source or self-hostable DDoS/L3-L4 filtering engines. Cloud scrubbing is context, not an apples-to-apples competitor. Sources checked 2026-11-25; vendor numbers are labeled.

| Engine | Enforcement | Detection | Best for | Caveat |
| :--- | :--- | :--- | :--- | :--- |
| [Tempesta xFW](https://github.com/tempesta-tech/xFW) | XDP + TC | L3-L7, HTTP | host/gateway/scrubbing | GPL-2.0, suite; ~200 Mpps vendor lab, not independent |
| [XDP-Firewall](https://github.com/gamemann/XDP-Firewall) | XDP | stateless | fast IP/range/port block | no detector |
| [xdp-filter](https://docs.redhat.com/en/documentation/red_hat_enterprise_linux/9/html/configuring_firewalls_and_packet_filters/using-xdp-filter-for-high-performance-traffic-filtering-to-prevent-ddos-attacks_firewall-packet-filters) | XDP | IP/MAC/port | distro built-in | not nftables parity; vendor 26M pps |
| [Couic](https://github.com/FCSC-FR/couic) | XDP | L7 | L7 filtering at line rate | L7, outside RamShield scope |
| [FastNetMon](https://github.com/pavel-odintsov/fastnetmon) + [prefixd](https://github.com/lance0/prefixd) | telemetry + BGP | flow/sFlow/SPAN | network/ISP | router integration, not one host |
| [CrowdSec](https://github.com/crowdsecurity/crowdsec) | bouncers | logs/behavior | host security stack | not an L3/L4 kernel dropper |
| Cloudflare | anycast | cloud ML | global edge | different product class |
| RamShield | XDP | EWMA/CUSUM/pulse/subnet | single-node Rust/MIT | small project; E1 evidence only |

## Independent context

- [Netdev 0x1A: xFW volumetric DDoS talk](https://netdevconf.info/0x1A/sessions/talk/tempesta-xfw-open-source-ebpf-based-volumetric-ddos-protection.html)
- [TUM 2025 network report: eBPF/XDP benchmark](https://www.net.in.tum.de/fileadmin/TUM/NET/NET-2025-11-3/NET-2025-11-3_04.pdf)
- [LWN: XDP tutorial](https://lwn.net/Articles/790243/)
- [Cloudflare Q3 2026 DDoS report](https://blog.cloudflare.com/ddos-threat-report-2026-q3)

## RamShield position

Transparent single-node Rust/MIT option. Not category leader. Claiming leadership is unsupported until independent reproduction and E2 data exist.