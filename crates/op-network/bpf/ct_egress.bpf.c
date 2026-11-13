// SPDX-License-Identifier: Apache-2.0
// Transparent container egress. Attached by the host to one container's cgroup.
//
// connect4: a container TCP connect() is redirected to the host-owned listener
//   on the container's loopback (127.0.0.1:PROXY_PORT). The real destination is
//   saved by socket cookie.
// sockops: when that connection is established, the real destination is re-keyed
//   by the loopback source port so the host egress service can recover it on
//   accept().
//
// The host egress service moves bytes with the kernel splice(2) path, so no
// byte is copied through a userspace buffer.
//
// UDP is not redirected (the container has no UDP egress path). IPv6 is not
// redirected (no IPv6 route in the container).
#include "vmlinux.h"
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_endian.h>

#define PROXY_PORT 3129          /* host-owned listener on the container's lo */
#define LOOPBACK   0x7f000001    /* 127.0.0.1, host byte order */

char LICENSE[] SEC("license") = "Apache-2.0";

struct dst {
    __u32 ip;   /* network byte order */
    __u16 port; /* network byte order */
};

/* original destination keyed by the connecting socket's cookie */
struct {
    __uint(type, BPF_MAP_TYPE_LRU_HASH);
    __uint(max_entries, 65536);
    __type(key, __u64);
    __type(value, struct dst);
} orig_by_cookie SEC(".maps");

/* original destination keyed by the loopback source port (host byte order),
 * read by the host egress service after accept() */
struct {
    __uint(type, BPF_MAP_TYPE_LRU_HASH);
    __uint(max_entries, 65536);
    __type(key, __u16);
    __type(value, struct dst);
    __uint(pinning, LIBBPF_PIN_BY_NAME);
} orig_by_srcport SEC(".maps");

SEC("cgroup/connect4")
int cg_connect4(struct bpf_sock_addr *ctx)
{
    if (ctx->protocol != IPPROTO_TCP)
        return 1;
    /* leave connections already aimed at the local proxy alone */
    if (ctx->user_ip4 == bpf_htonl(LOOPBACK) &&
        ctx->user_port == bpf_htons(PROXY_PORT))
        return 1;

    struct dst d = {
        .ip = ctx->user_ip4,
        .port = ctx->user_port,
    };
    __u64 cookie = bpf_get_socket_cookie(ctx);
    bpf_map_update_elem(&orig_by_cookie, &cookie, &d, BPF_ANY);

    ctx->user_ip4 = bpf_htonl(LOOPBACK);
    ctx->user_port = bpf_htons(PROXY_PORT);
    return 1;
}

SEC("sockops")
int cg_sockops(struct bpf_sock_ops *ops)
{
    if (ops->op == BPF_SOCK_OPS_ACTIVE_ESTABLISHED_CB) {
        __u64 cookie = bpf_get_socket_cookie(ops);
        struct dst *d = bpf_map_lookup_elem(&orig_by_cookie, &cookie);
        if (d) {
            __u16 sport = (__u16)ops->local_port; /* host byte order */
            bpf_map_update_elem(&orig_by_srcport, &sport, d, BPF_ANY);
            bpf_map_delete_elem(&orig_by_cookie, &cookie);
        }
    }
    return 0;
}
