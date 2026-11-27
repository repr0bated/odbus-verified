// SPDX-License-Identifier: Apache-2.0
// Preserve the kernel identity of accepted browser sockets without reading bytes.
#include "vmlinux.h"
#include <bpf/bpf_helpers.h>

#define BROWSER_PORT 8080
#define PENDING_TTL_NS (15ULL * 1000000000ULL)

struct browser_route_value {
    __u8 handle[16];
    __u64 expires_ns;
    __u32 ready;
    __u32 reserved;
};

struct {
    __uint(type, BPF_MAP_TYPE_LRU_HASH);
    __uint(max_entries, 4096);
    __type(key, __u64);
    __type(value, struct browser_route_value);
    __uint(pinning, LIBBPF_PIN_BY_NAME);
} browser_routes SEC(".maps");

SEC("sockops")
int browser_ingress(struct bpf_sock_ops *ops)
{
    __u64 cookie;

    if (ops->op == BPF_SOCK_OPS_PASSIVE_ESTABLISHED_CB) {
        struct browser_route_value pending = {};

        if (ops->local_port != BROWSER_PORT)
            return 0;
        cookie = bpf_get_socket_cookie(ops);
        if (!cookie)
            return 0;
        pending.expires_ns = bpf_ktime_get_ns() + PENDING_TTL_NS;
        bpf_map_update_elem(&browser_routes, &cookie, &pending, BPF_NOEXIST);
        bpf_sock_ops_cb_flags_set(ops, BPF_SOCK_OPS_STATE_CB_FLAG);
        return 0;
    }

    if (ops->op == BPF_SOCK_OPS_STATE_CB) {
        cookie = bpf_get_socket_cookie(ops);
        if (cookie && ops->args[1] == BPF_TCP_CLOSE)
            bpf_map_delete_elem(&browser_routes, &cookie);
    }
    return 0;
}

char LICENSE[] SEC("license") = "Apache-2.0";
