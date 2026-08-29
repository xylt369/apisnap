// src/ebpf/traffic_capture.bpf.c
// Target: BPF bytecode loaded into Linux TC (Traffic Control) egress/ingress hook
#include <linux/bpf.h>
#include <linux/if_ether.h>
#include <linux/ip.h>
#include <linux/tcp.h>
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_endian.h>

#define MAX_PAYLOAD_CAPTURE 4096

/* Ring buffer map for zero-lock kernel-to-userspace stream */
struct {
    __uint(type, BPF_MAP_TYPE_RINGBUF);
    __uint(max_entries, 1 << 24); /* 16MB Ring Buffer */
} traffic_events SEC(".maps");

struct captured_packet {
    __u32 src_ip;
    __u32 dst_ip;
    __u16 src_port;
    __u16 dst_port;
    __u32 payload_len;
    __u8  payload[MAX_PAYLOAD_CAPTURE];
};

SEC("tc")
int capture_egress(struct __sk_buff *skb) {
    void *data_end = (void *)(long)skb->data_end;
    void *data = (void *)(long)skb->data;

    struct ethhdr *eth = data;
    if ((void *)(eth + 1) > data_end)
        return TC_ACT_OK;

    if (bpf_ntohs(eth->h_proto) != ETH_P_IP)
        return TC_ACT_OK;

    struct iphdr *ip = (void *)(eth + 1);
    if ((void *)(ip + 1) > data_end)
        return TC_ACT_OK;

    if (ip->protocol != IPPROTO_TCP)
        return TC_ACT_OK;

    struct tcphdr *tcp = (void *)ip + (ip->ihl * 4);
    if ((void *)(tcp + 1) > data_end)
        return TC_ACT_OK;

    __u16 dst_port = bpf_ntohs(tcp->dest);
    if (dst_port != 80 && dst_port != 8080 && dst_port != 3000 && dst_port != 8000)
        return TC_ACT_OK;

    void *payload = (void *)tcp + (tcp->doff * 4);
    __u32 payload_len = (__u32)(data_end - payload);
    if (payload <= data_end && payload_len > 0) {
        struct captured_packet *evt;
        __u32 cap_len = payload_len > MAX_PAYLOAD_CAPTURE ? MAX_PAYLOAD_CAPTURE : payload_len;

        evt = bpf_ringbuf_reserve(&traffic_events, sizeof(*evt), 0);
        if (!evt)
            return TC_ACT_OK;

        evt->src_ip = ip->saddr;
        evt->dst_ip = ip->daddr;
        evt->src_port = bpf_ntohs(tcp->source);
        evt->dst_port = dst_port;
        evt->payload_len = cap_len;

        if (payload + cap_len <= data_end) {
            bpf_probe_read_kernel(&evt->payload, cap_len, payload);
        }

        bpf_ringbuf_submit(evt, 0);
    }

    return TC_ACT_OK;
}

char _license[] SEC("license") = "GPL";
