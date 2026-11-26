# FAQ

1. **Kernel?** Linux with XDP support; tested on 6.8.0. Check `docs/SUPPORT.md`.
2. **Root?** Yes for XDP attach; userspace demo runs unprivileged.
3. **L7 / WAF?** No. See `docs/NOT_FOR.md`.
4. **Cloud account?** None. No feed, no subscription.
5. **State after crash?** WAL + checkpoint; see `docs/OPERATIONS.md`.
6. **Raw benchmark data?** Not in-tree. Historical numbers are E1.
7. **False positives?** One run: 0 of 200 good IPs blocked. Not an estimate; upper bound 1.5%.
8. **Saturated uplink?** Cannot help. Upstream ISP must act.
9. **Install?** See `docs/INSTALL.md`.
10. **Contribute?** Start with `docs/GOOD_FIRST_ISSUES.md` and `docs/REPRODUCTION.md`.
