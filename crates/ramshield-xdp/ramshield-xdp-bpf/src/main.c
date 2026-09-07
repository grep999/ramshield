#include <linux/bpf.h>
#include <linux/if_ether.h>
#include <linux/ip.h>
#include <linux/ipv6.h>
#include <linux/in.h>
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_endian.h>

struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 102400);
    __type(key, __u64[2]);
    __type(value, __u8);
} BLOCKLIST SEC(".maps");

/* IPv6 blocklist — dedicated map (plan D2): a 16-byte v6 key can collide
 * numerically with the v4 shape (octets at bytes 0..4, zero pad), so
 * families are isolated by map, not by key bytes. Same 16-byte key type:
 * the raw saddr octets, memcpy'd straight — memory == wire order, which is
 * exactly what Rust BlocklistKey::from_ip(V6) serializes (from_le_bytes). */
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 102400);
    __type(key, __u64[2]);
    __type(value, __u8);
} BLOCKLIST6 SEC(".maps");

SEC("xdp")
int ramshield_xdp(struct xdp_md *ctx) {
    void *data_end = (void *)(long)ctx->data_end;
    void *data = (void *)(long)ctx->data;

    struct ethhdr *eth = data;
    if ((void *)(eth + 1) > data_end) {
        return XDP_PASS;
    }
    if (eth->h_proto == bpf_htons(ETH_P_IPV6)) {
        struct ipv6hdr *ip6 = (void *)(eth + 1);
        if ((void *)(ip6 + 1) > data_end) {
            return XDP_PASS;
        }
        __u64 key[2];
        __builtin_memcpy(key, ip6->saddr.s6_addr, sizeof(key)); /* raw wire bytes */
        if (bpf_map_lookup_elem(&BLOCKLIST6, &key)) {
            return XDP_DROP;
        }
        return XDP_PASS;
    }
    if (eth->h_proto != bpf_htons(ETH_P_IP)) {
        return XDP_PASS;
    }

    struct iphdr *ip = (void *)(eth + 1);
    if ((void *)(ip + 1) > data_end) {
        return XDP_PASS;
    }

    __u32 src_ip = ip->saddr;
    __u64 key[2] = {0, 0};
    key[0] = src_ip;
    if (bpf_map_lookup_elem(&BLOCKLIST, &key)) {
        return XDP_DROP;
    }
    return XDP_PASS;
}

char _license[] SEC("license") = "GPL";
