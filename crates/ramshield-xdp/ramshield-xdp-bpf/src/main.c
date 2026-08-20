#include <linux/bpf.h>
#include <linux/if_ether.h>
#include <linux/ip.h>
#include <linux/in.h>
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_endian.h>

struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 102400);
    __type(key, __u64[2]);
    __type(value, __u8);
} BLOCKLIST SEC(".maps");

SEC("xdp")
int ramshield_xdp(struct xdp_md *ctx) {
    void *data_end = (void *)(long)ctx->data_end;
    void *data = (void *)(long)ctx->data;

    struct ethhdr *eth = data;
    if ((void *)(eth + 1) > data_end) {
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
