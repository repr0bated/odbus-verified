// SPDX-License-Identifier: Apache-2.0
// Transparent, NIC-less container egress. Attached by the host to one
// container's cgroup. No Xray, no HTTP proxy, no proxy environment.
//
// connect4: a container TCP connect() to a non-local destination is
//   redirected to the host-owned listener on the container's loopback
//   (127.0.0.1:PROXY_PORT). The real destination is saved by socket cookie.
// sockops: when that connection is established, the real destination is
//   re-keyed by the loopback source port so the host egress service can
//   recover it on accept(). The host service then dials the destination
//   directly from the host network namespace (fd passed back over a
//   socketpair) and moves bytes with splice(2): no userspace copy, no
//   intermediate proxy process.
//
// Local destinations are never redirected: the whole loopback range and
// the container's own door alias (10.0.0.2 on lo) stay in-container.
// Private/non-local destinations are still redirected; the host loader
// refuses to dial them, so a private target fails closed at dial time
// instead of reaching a host or backend service.
//
// UDP is not redirected here; the loader serves DNS itself on
// 127.0.0.2:53 inside the container netns. IPv6 is not redirected (the
// loader refuses IPv6 upstreams and the container has no IPv6 route).
#include "vmlinux.h"
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_endian.h>

#define PROXY_PORT 3129          /* host-owned listener on the container's lo */
#define DOOR_ALIAS 0x0a000002    /* 10.0.0.2, host byte order: door on guest lo */

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
 * read by the host egress service after accept(). Not pinned: each loader
 * owns its own maps, so concurrent containers cannot collide on a shared
 * pinned map (source ports repeat across netns). */
struct {
    __uint(type, BPF_MAP_TYPE_LRU_HASH);
    __uint(max_entries, 65536);
    __type(key, __u16);
    __type(value, struct dst);
} orig_by_srcport SEC(".maps");

SEC("cgroup/connect4")
int cg_connect4(struct bpf_sock_addr *ctx)
{
    if (ctx->protocol != IPPROTO_TCP)
        return 1;
    /* never redirect local traffic: loopback (proxy/DNS/door listeners in
     * the guest) and the guest's own door alias on lo */
    if ((ctx->user_ip4 & bpf_htonl(0xff000000)) == bpf_htonl(0x7f000000))
        return 1;
    if (ctx->user_ip4 == bpf_htonl(DOOR_ALIAS))
        return 1;

    struct dst d = {
        .ip = ctx->user_ip4,
        .port = ctx->user_port,
    };
    __u64 cookie = bpf_get_socket_cookie(ctx);
    bpf_map_update_elem(&orig_by_cookie, &cookie, &d, BPF_ANY);

    ctx->user_ip4 = bpf_htonl(0x7f000001);
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
