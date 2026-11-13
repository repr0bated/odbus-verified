// SPDX-License-Identifier: Apache-2.0
#define _GNU_SOURCE
// ct-egress-bpf — transparent egress for one NIC-less container.
//
//   ct-egress-bpf <container-init-pid> <cgroup-path> <xray-http-sock>
//
// Loads ct_egress.bpf.o, attaches cgroup/connect4 and sockops to the
// container's cgroup, then holds a listener on 127.0.0.1:PROXY_PORT inside the
// container's network namespace. The connect4 program redirects every container
// TCP connect() to that listener; sockops records the original destination by
// loopback source port. For each accepted connection this service looks the
// destination up, opens an HTTP CONNECT to the host Xray proxy (Unix socket),
// and moves bytes with splice(2) (no userspace copy). The container needs no
// NIC and no proxy environment.
#include <bpf/libbpf.h>
#include <bpf/bpf.h>
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <sched.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>
#include <sys/wait.h>

#define PROXY_PORT 3129
#define PIN_SRCPORT "/sys/fs/bpf/ct_egress_orig_by_srcport"

static const char *xray_sock;

struct dst { uint32_t ip; uint16_t port; };

static int enter_netns(int pid) {
    char path[64];
    snprintf(path, sizeof path, "/proc/%d/ns/net", pid);
    int fd = open(path, O_RDONLY | O_CLOEXEC);
    if (fd < 0) return -1;
    int rc = setns(fd, CLONE_NEWNET);
    close(fd);
    return rc;
}

/* splice one direction through a pipe: no userspace buffer */
struct pump { int from, to; };
static void *pump(void *arg) {
    struct pump *p = arg;
    int pfd[2];
    if (pipe(pfd) < 0) return NULL;
    for (;;) {
        ssize_t n = splice(p->from, NULL, pfd[1], NULL, 1 << 16,
                           SPLICE_F_MOVE | SPLICE_F_MORE);
        if (n <= 0) break;
        while (n > 0) {
            ssize_t m = splice(pfd[0], NULL, p->to, NULL, n,
                               SPLICE_F_MOVE | SPLICE_F_MORE);
            if (m <= 0) { n = -1; break; }
            n -= m;
        }
        if (n < 0) break;
    }
    shutdown(p->to, SHUT_WR);
    close(pfd[0]); close(pfd[1]);
    free(p);
    return NULL;
}

static int connect_xray(void) {
    int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (fd < 0) return -1;
    struct sockaddr_un sa = { .sun_family = AF_UNIX };
    strncpy(sa.sun_path, xray_sock, sizeof sa.sun_path - 1);
    if (connect(fd, (void *)&sa, sizeof sa) < 0) { close(fd); return -1; }
    return fd;
}

/* HTTP CONNECT handshake to the Xray http proxy; returns 0 on 200 */
static int http_connect(int up, struct dst d) {
    char ip[INET_ADDRSTRLEN];
    inet_ntop(AF_INET, &d.ip, ip, sizeof ip);
    uint16_t port = ntohs(d.port);
    char req[128];
    int n = snprintf(req, sizeof req,
        "CONNECT %s:%u HTTP/1.1\r\nHost: %s:%u\r\n\r\n", ip, port, ip, port);
    if (write(up, req, n) != n) return -1;
    char buf[256]; int off = 0;
    while (off < (int)sizeof buf - 1) {
        ssize_t r = read(up, buf + off, sizeof buf - 1 - off);
        if (r <= 0) return -1;
        off += r;
        buf[off] = 0;
        if (strstr(buf, "\r\n\r\n")) break;
    }
    return strncmp(buf, "HTTP/1.1 200", 12) == 0 ||
           strncmp(buf, "HTTP/1.0 200", 12) == 0 ? 0 : -1;
}

static void handle(int client, int srcport_map) {
    struct sockaddr_in peer; socklen_t pl = sizeof peer;
    if (getpeername(client, (void *)&peer, &pl) < 0) { close(client); return; }
    uint16_t sport = ntohs(peer.sin_port);
    struct dst d;
    /* sockops writes the destination at ESTABLISHED, which can land just after
     * accept(); retry briefly before giving up. */
    int found = -1;
    for (int i = 0; i < 100; i++) {
        if (bpf_map_lookup_elem(srcport_map, &sport, &d) == 0) { found = 0; break; }
        usleep(1000);
    }
    if (found < 0) { fprintf(stderr, "no dst for srcport %u\n", sport); close(client); return; }
    int up = connect_xray();
    if (up < 0) { fprintf(stderr, "connect_xray: %s\n", strerror(errno)); close(client); return; }
    if (http_connect(up, d) < 0) { fprintf(stderr, "http_connect failed\n"); close(up); close(client); return; }

    pthread_t t1, t2;
    struct pump *a = malloc(sizeof *a), *b = malloc(sizeof *b);
    *a = (struct pump){ client, up };
    *b = (struct pump){ up, client };
    pthread_create(&t1, NULL, pump, a);
    pthread_create(&t2, NULL, pump, b);
    pthread_join(t1, NULL); pthread_join(t2, NULL);
    close(up); close(client);
}

int main(int argc, char **argv) {
    if (argc != 4) {
        fprintf(stderr, "usage: %s <pid> <cgroup-path> <xray-http-sock>\n", argv[0]);
        return 2;
    }
    int pid = atoi(argv[1]);
    const char *cgroup = argv[2];
    xray_sock = argv[3];

    struct bpf_object *obj = bpf_object__open_file(
        "/usr/local/lib/op-dbus/ct_egress.bpf.o", NULL);
    if (!obj) { fprintf(stderr, "open bpf: %s\n", strerror(errno)); return 1; }
    if (bpf_object__load(obj)) { fprintf(stderr, "load bpf: %s\n", strerror(errno)); return 1; }

    int cg = open(cgroup, O_DIRECTORY | O_RDONLY);
    if (cg < 0) { fprintf(stderr, "open cgroup %s: %s\n", cgroup, strerror(errno)); return 1; }
    struct bpf_program *c4 = bpf_object__find_program_by_name(obj, "cg_connect4");
    struct bpf_program *so = bpf_object__find_program_by_name(obj, "cg_sockops");
    if (bpf_prog_attach(bpf_program__fd(c4), cg, BPF_CGROUP_INET4_CONNECT, BPF_F_ALLOW_MULTI) ||
        bpf_prog_attach(bpf_program__fd(so), cg, BPF_CGROUP_SOCK_OPS, BPF_F_ALLOW_MULTI)) {
        fprintf(stderr, "attach: %s\n", strerror(errno));
        return 1;
    }
    int srcport_map = bpf_object__find_map_fd_by_name(obj, "orig_by_srcport");

    /* listener inside the container's network namespace */
    if (enter_netns(pid)) { fprintf(stderr, "setns: %s\n", strerror(errno)); return 1; }
    int lsn = socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0);
    int one = 1; setsockopt(lsn, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
    struct sockaddr_in a = { .sin_family = AF_INET,
        .sin_addr.s_addr = htonl(0x7f000001), .sin_port = htons(PROXY_PORT) };
    if (bind(lsn, (void *)&a, sizeof a) || listen(lsn, 128)) {
        fprintf(stderr, "listen in netns: %s\n", strerror(errno));
        return 1;
    }
    fprintf(stderr, "ct-egress-bpf: pid %d cgroup %s -> %s, listening 127.0.0.1:%d in netns\n",
            pid, cgroup, xray_sock, PROXY_PORT);

    for (;;) {
        int client = accept4(lsn, NULL, NULL, SOCK_CLOEXEC);
        if (client < 0) { if (errno == EINTR) continue; break; }
        if (!fork()) { close(lsn); handle(client, srcport_map); _exit(0); }
        close(client);
        while (waitpid(-1, NULL, WNOHANG) > 0) {}
    }
    return 0;
}
