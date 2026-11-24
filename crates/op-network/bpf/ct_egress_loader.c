// SPDX-License-Identifier: Apache-2.0
#define _GNU_SOURCE
// ct-egress-bpf — transparent, NIC-less egress for one container.
// No Xray, no HTTP proxy, no socat, no proxy environment.
//
//   ct-egress-bpf <container-init-pid> [upstream-dns-ipv4[:port]]
//
// Loads ct_egress.bpf.o, attaches cgroup/connect4 + sockops to the
// container's cgroup (discovered from /proc/<pid>/cgroup), then serves
// inside the container's network namespace:
//   * TCP 127.0.0.1:3129 — every non-local guest TCP connect() lands here
//     (BPF-redirected). The original destination is recovered from the
//     BPF map; a host-netns helper process dials it directly (public
//     destinations only) and passes the connected fd back over a
//     socketpair; bytes then move with splice(2), no userspace copy.
//   * UDP+TCP 127.0.0.2:53 — DNS stub. Queries are framed over a second
//     socketpair to the host-netns helper, which resolves them against
//     the upstream resolver from the host namespace.
//
// The guest needs no NIC, no route, no proxy variables and no in-guest
// relay service: only /etc/resolv.conf pointing at 127.0.0.2 (directly,
// or via an in-guest NextDNS client whose own DoH TCP is itself carried
// by this egress). Private/loopback/link-local destinations are refused
// before dial, in the helper as well as checked again here.
#include <bpf/libbpf.h>
#include <bpf/bpf.h>
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <sched.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <sys/uio.h>
#include <sys/wait.h>
#include <unistd.h>

#define PROXY_PORT 3129
#define DNS_ADDR "127.0.0.2"
#define MAX_DNS 16384
#define MAX_WAITERS 4096

struct dst { uint32_t ip; uint16_t port; }; /* both network byte order */

static char upstream_dns[64] = "1.1.1.1";
static int upstream_port = 53;

/* ---------- small io helpers ---------- */
static int read_all(int fd, void *buf, size_t n) {
    char *p = buf;
    while (n) { ssize_t r = read(fd, p, n); if (r <= 0) return -1; p += r; n -= (size_t)r; }
    return 0;
}
static int write_all(int fd, const void *buf, size_t n) {
    const char *p = buf;
    while (n) { ssize_t r = write(fd, p, n); if (r <= 0) return -1; p += r; n -= (size_t)r; }
    return 0;
}

/* ---------- destination policy (same set as the old ct_egress.nft) ---------- */
static int ip_blocked(uint32_t ip) { /* ip in host byte order */
    static const struct { uint32_t base; int plen; } blocked[] = {
        {0x00000000, 8},  /* 0.0.0.0/8       */
        {0x0a000000, 8},  /* 10.0.0.0/8      */
        {0x64400000, 10}, /* 100.64.0.0/10   */
        {0x7f000000, 8},  /* 127.0.0.0/8     */
        {0xa9fe0000, 16}, /* 169.254.0.0/16  */
        {0xac100000, 12}, /* 172.16.0.0/12   */
        {0xc0000000, 24}, /* 192.0.0.0/24    */
        {0xc0a80000, 16}, /* 192.168.0.0/16   */
        {0xc6120000, 15}, /* 198.18.0.0/15   */
        {0xe0000000, 3},  /* 224.0.0.0/3     */
    };
    for (size_t i = 0; i < sizeof blocked / sizeof blocked[0]; i++) {
        uint32_t mask = blocked[i].plen ? (~0u << (32 - blocked[i].plen)) : 0;
        if ((ip & mask) == (blocked[i].base & mask)) return 1;
    }
    return 0;
}

/* ---------- host-netns helper (parent process) ---------- */
static int dns_chan = -1, egress_chan = -1;
static pthread_mutex_t dns_chan_wr = PTHREAD_MUTEX_INITIALIZER;
static pthread_mutex_t egress_chan_wr = PTHREAD_MUTEX_INITIALIZER;

struct dns_req { uint32_t id; uint16_t len; unsigned char payload[MAX_DNS]; };
static void *dns_query_run(void *arg) {
    struct dns_req *req = arg;
    int s = socket(AF_INET, SOCK_DGRAM | SOCK_CLOEXEC, 0);
    if (s >= 0) {
        struct timeval tv = { .tv_sec = 5 };
        setsockopt(s, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof tv);
        struct sockaddr_in up = { .sin_family = AF_INET, .sin_port = htons((uint16_t)upstream_port) };
        if (inet_pton(AF_INET, upstream_dns, &up.sin_addr) == 1 &&
            sendto(s, req->payload, req->len, 0, (void *)&up, sizeof up) >= 0) {
            unsigned char resp[MAX_DNS];
            ssize_t n = recv(s, resp, sizeof resp, 0);
            if (n > 0) {
                unsigned char hdr[6];
                uint32_t nid = htonl(req->id); uint16_t nlen = htons((uint16_t)n);
                memcpy(hdr, &nid, 4); memcpy(hdr + 4, &nlen, 2);
                pthread_mutex_lock(&dns_chan_wr);
                if (write_all(dns_chan, hdr, 6) == 0) write_all(dns_chan, resp, (size_t)n);
                pthread_mutex_unlock(&dns_chan_wr);
            }
        }
        close(s);
    }
    free(req);
    return NULL;
}

static void *host_dns_loop(void *unused) {
    (void)unused;
    for (;;) {
        unsigned char hdr[6];
        if (read_all(dns_chan, hdr, 6) < 0) break;
        struct dns_req *req = malloc(sizeof *req);
        if (!req) break;
        uint32_t nid; uint16_t nlen;
        memcpy(&nid, hdr, 4); memcpy(&nlen, hdr + 4, 2);
        req->id = ntohl(nid); req->len = ntohs(nlen);
        if (req->len > MAX_DNS || read_all(dns_chan, req->payload, req->len) < 0) { free(req); break; }
        pthread_t t;
        if (pthread_create(&t, NULL, dns_query_run, req) == 0) pthread_detach(t);
        else free(req);
    }
    return NULL;
}

struct dial_req { uint32_t id; uint32_t ip; uint16_t port; }; /* ip/port network order */
static void *dial_run(void *arg) {
    struct dial_req *req = arg;
    int out = -1;
    uint32_t ip = ntohl(req->ip);
    if (!ip_blocked(ip)) {
        out = socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0);
        if (out >= 0) {
            int flags = fcntl(out, F_GETFL, 0);
            fcntl(out, F_SETFL, flags | O_NONBLOCK);
            struct sockaddr_in sa = { .sin_family = AF_INET, .sin_port = req->port, .sin_addr.s_addr = req->ip };
            int rc = connect(out, (void *)&sa, sizeof sa);
            if (rc < 0 && errno == EINPROGRESS) {
                fd_set wf; FD_ZERO(&wf); FD_SET(out, &wf);
                struct timeval tv = { .tv_sec = 10 };
                rc = select(out + 1, NULL, &wf, NULL, &tv) > 0 ? 0 : -1;
                if (rc == 0) { int err = 0; socklen_t el = sizeof err; getsockopt(out, SOL_SOCKET, SO_ERROR, &err, &el); if (err) rc = -1; }
            }
            if (rc < 0) { close(out); out = -1; }
            else fcntl(out, F_SETFL, flags);
        }
    }
    /* reply: one datagram [id][status], fd attached on success */
    unsigned char msg[5];
    uint32_t nid = htonl(req->id); memcpy(msg, &nid, 4); msg[4] = out >= 0 ? 0 : 1;
    struct iovec iov = { .iov_base = msg, .iov_len = sizeof msg };
    char cmsg[CMSG_SPACE(sizeof(int))];
    struct msghdr mh = { .msg_iov = &iov, .msg_iovlen = 1 };
    if (out >= 0) {
        mh.msg_control = cmsg; mh.msg_controllen = sizeof cmsg;
        struct cmsghdr *c = CMSG_FIRSTHDR(&mh);
        c->cmsg_level = SOL_SOCKET; c->cmsg_type = SCM_RIGHTS; c->cmsg_len = CMSG_LEN(sizeof(int));
        memcpy(CMSG_DATA(c), &out, sizeof out);
    }
    pthread_mutex_lock(&egress_chan_wr);
    sendmsg(egress_chan, &mh, 0);
    pthread_mutex_unlock(&egress_chan_wr);
    if (out >= 0) close(out);
    free(req);
    return NULL;
}

static void *host_egress_loop(void *unused) {
    (void)unused;
    for (;;) {
        unsigned char buf[10];
        ssize_t n = recv(egress_chan, buf, sizeof buf, 0);
        if (n != 10) { if (n <= 0) break; continue; }
        struct dial_req *req = malloc(sizeof *req);
        if (!req) break;
        uint32_t nid; memcpy(&nid, buf, 4); req->id = ntohl(nid);
        memcpy(&req->ip, buf + 4, 4); memcpy(&req->port, buf + 8, 2);
        pthread_t t;
        if (pthread_create(&t, NULL, dial_run, req) == 0) pthread_detach(t);
        else free(req);
    }
    return NULL;
}

/* ---------- guest-netns server (child process) ---------- */
static int guest_dns_chan = -1, guest_egress_chan = -1;
static pthread_mutex_t guest_dns_wr = PTHREAD_MUTEX_INITIALIZER;
static pthread_mutex_t guest_egress_wr = PTHREAD_MUTEX_INITIALIZER;

/* DNS waiters */
struct waiter { int used; int kind; struct sockaddr_in udp_addr; int tcp_fd; };
static struct waiter waiters[MAX_WAITERS];
static pthread_mutex_t waiters_mu = PTHREAD_MUTEX_INITIALIZER;
static uint32_t next_id = 1;
static int dns_udp_fd = -1;

static uint32_t waiter_add(int kind, const struct sockaddr_in *addr, int tcp_fd) {
    pthread_mutex_lock(&waiters_mu);
    for (int i = 0; i < MAX_WAITERS; i++) {
        uint32_t id = next_id++;
        if (!next_id) next_id = 1;
        int slot = (int)(id % MAX_WAITERS);
        if (!waiters[slot].used) {
            waiters[slot].used = 1; waiters[slot].kind = kind; waiters[slot].tcp_fd = tcp_fd;
            if (addr) waiters[slot].udp_addr = *addr;
            pthread_mutex_unlock(&waiters_mu);
            return id;
        }
    }
    pthread_mutex_unlock(&waiters_mu);
    return 0;
}
static int waiter_take(uint32_t id, struct waiter *out) {
    int slot = (int)(id % MAX_WAITERS);
    pthread_mutex_lock(&waiters_mu);
    if (!waiters[slot].used) { pthread_mutex_unlock(&waiters_mu); return -1; }
    *out = waiters[slot]; waiters[slot].used = 0;
    pthread_mutex_unlock(&waiters_mu);
    return 0;
}
static void dns_send_request(uint32_t id, const unsigned char *payload, uint16_t len) {
    unsigned char hdr[6];
    uint32_t nid = htonl(id); uint16_t nlen = htons(len);
    memcpy(hdr, &nid, 4); memcpy(hdr + 4, &nlen, 2);
    pthread_mutex_lock(&guest_dns_wr);
    if (write_all(guest_dns_chan, hdr, 6) == 0) write_all(guest_dns_chan, payload, len);
    pthread_mutex_unlock(&guest_dns_wr);
}
static void *dns_response_loop(void *unused) {
    (void)unused;
    for (;;) {
        unsigned char hdr[6];
        if (read_all(guest_dns_chan, hdr, 6) < 0) break;
        uint32_t nid; uint16_t nlen;
        memcpy(&nid, hdr, 4); memcpy(&nlen, hdr + 4, 2);
        uint32_t id = ntohl(nid); uint16_t len = ntohs(nlen);
        unsigned char *payload = malloc(len ? len : 1);
        if (!payload || len > MAX_DNS || read_all(guest_dns_chan, payload, len) < 0) { free(payload); break; }
        struct waiter w;
        if (waiter_take(id, &w) == 0) {
            if (w.kind == 1 && dns_udp_fd >= 0)
                sendto(dns_udp_fd, payload, len, 0, (void *)&w.udp_addr, sizeof w.udp_addr);
            else if (w.kind == 2) {
                unsigned char lh[2] = { (unsigned char)(len >> 8), (unsigned char)len };
                write_all(w.tcp_fd, lh, 2); write_all(w.tcp_fd, payload, len);
                close(w.tcp_fd);
            }
        }
        free(payload);
    }
    return NULL;
}
static void *dns_udp_loop(void *unused) {
    (void)unused;
    unsigned char buf[MAX_DNS];
    for (;;) {
        struct sockaddr_in cli; socklen_t cl = sizeof cli;
        ssize_t n = recvfrom(dns_udp_fd, buf, sizeof buf, 0, (void *)&cli, &cl);
        if (n <= 0) continue;
        uint32_t id = waiter_add(1, &cli, -1);
        if (id) dns_send_request(id, buf, (uint16_t)n);
    }
    return NULL;
}
struct tcp_dns { int fd; };
static void *dns_tcp_conn(void *arg) {
    struct tcp_dns *c = arg;
    unsigned char lh[2];
    if (read_all(c->fd, lh, 2) == 0) {
        uint16_t len = (uint16_t)((lh[0] << 8) | lh[1]);
        unsigned char *buf = malloc(len ? len : 1);
        if (buf && len <= MAX_DNS && read_all(c->fd, buf, len) == 0) {
            uint32_t id = waiter_add(2, NULL, c->fd);
            if (id) { dns_send_request(id, buf, len); free(buf); free(c); return NULL; }
        }
        free(buf);
    }
    close(c->fd); free(c);
    return NULL;
}
static void *dns_tcp_loop(void *arg) {
    int lsn = (int)(intptr_t)arg;
    for (;;) {
        int fd = accept4(lsn, NULL, NULL, SOCK_CLOEXEC);
        if (fd < 0) continue;
        struct tcp_dns *c = malloc(sizeof *c); if (!c) { close(fd); continue; }
        c->fd = fd;
        pthread_t t; if (pthread_create(&t, NULL, dns_tcp_conn, c) == 0) pthread_detach(t); else { close(fd); free(c); }
    }
    return NULL;
}

/* egress fd waiters */
struct ewaiter { int used; int fd; int status; pthread_cond_t cond; };
static struct ewaiter ewaiters[MAX_WAITERS];
static pthread_mutex_t ewaiters_mu = PTHREAD_MUTEX_INITIALIZER;
static void *egress_response_loop(void *unused) {
    (void)unused;
    for (;;) {
        unsigned char buf[5];
        char cmsg[CMSG_SPACE(sizeof(int))];
        struct iovec iov = { .iov_base = buf, .iov_len = sizeof buf };
        struct msghdr mh = { .msg_iov = &iov, .msg_iovlen = 1, .msg_control = cmsg, .msg_controllen = sizeof cmsg };
        ssize_t n = recvmsg(guest_egress_chan, &mh, 0);
        if (n != 5) { if (n <= 0) break; continue; }
        uint32_t nid; memcpy(&nid, buf, 4); uint32_t id = ntohl(nid);
        int fd = -1;
        for (struct cmsghdr *c = CMSG_FIRSTHDR(&mh); c; c = CMSG_NXTHDR(&mh, c))
            if (c->cmsg_level == SOL_SOCKET && c->cmsg_type == SCM_RIGHTS) memcpy(&fd, CMSG_DATA(c), sizeof fd);
        int slot = (int)(id % MAX_WAITERS);
        pthread_mutex_lock(&ewaiters_mu);
        if (ewaiters[slot].used) {
            ewaiters[slot].fd = fd;
            ewaiters[slot].status = buf[4] == 0 ? 0 : 2; /* 1=pending, 0=ok, 2=failed */
            pthread_cond_signal(&ewaiters[slot].cond);
        } else if (fd >= 0) close(fd);
        pthread_mutex_unlock(&ewaiters_mu);
    }
    return NULL;
}
static int request_dial(uint32_t ip, uint16_t port) { /* network order in/out, fd or -1 */
    pthread_mutex_lock(&ewaiters_mu);
    uint32_t id = next_id++; if (!next_id) next_id = 1;
    int slot = (int)(id % MAX_WAITERS);
    if (ewaiters[slot].used) { pthread_mutex_unlock(&ewaiters_mu); return -1; }
    ewaiters[slot].used = 1; ewaiters[slot].fd = -1; ewaiters[slot].status = 1;
    pthread_cond_init(&ewaiters[slot].cond, NULL);
    pthread_mutex_unlock(&ewaiters_mu);

    unsigned char req[10]; uint32_t nid = htonl(id);
    memcpy(req, &nid, 4); memcpy(req + 4, &ip, 4); memcpy(req + 8, &port, 2);
    pthread_mutex_lock(&guest_egress_wr);
    send(guest_egress_chan, req, sizeof req, 0);
    pthread_mutex_unlock(&guest_egress_wr);

    pthread_mutex_lock(&ewaiters_mu);
    struct timespec ts; clock_gettime(CLOCK_REALTIME, &ts); ts.tv_sec += 15;
    while (ewaiters[slot].status == 1) {
        if (pthread_cond_timedwait(&ewaiters[slot].cond, &ewaiters_mu, &ts) != 0) break;
    }
    int fd = (ewaiters[slot].status == 0) ? ewaiters[slot].fd : -1;
    ewaiters[slot].used = 0;
    pthread_cond_destroy(&ewaiters[slot].cond);
    pthread_mutex_unlock(&ewaiters_mu);
    return fd;
}

/* splice pump, as before: no userspace buffer */
struct pump { int from, to; };
static void *pump(void *arg) {
    struct pump *p = arg;
    int pfd[2];
    if (pipe(pfd) < 0) return NULL;
    for (;;) {
        ssize_t n = splice(p->from, NULL, pfd[1], NULL, 1 << 16, SPLICE_F_MOVE | SPLICE_F_MORE);
        if (n <= 0) break;
        while (n > 0) {
            ssize_t m = splice(pfd[0], NULL, p->to, NULL, n, SPLICE_F_MOVE | SPLICE_F_MORE);
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

static int srcport_map_fd = -1;
struct conn { int client; };
static void *handle_conn(void *arg) {
    struct conn *c = arg;
    int client = c->client; free(c);
    struct sockaddr_in peer; socklen_t pl = sizeof peer;
    if (getpeername(client, (void *)&peer, &pl) < 0) { close(client); return NULL; }
    uint16_t sport = ntohs(peer.sin_port);
    struct dst d; int found = -1;
    for (int i = 0; i < 100; i++) {
        if (bpf_map_lookup_elem(srcport_map_fd, &sport, &d) == 0) { found = 0; break; }
        usleep(1000);
    }
    if (found < 0) { fprintf(stderr, "ct-egress-bpf: no dst for srcport %u\n", sport); close(client); return NULL; }
    if (ip_blocked(ntohl(d.ip))) {
        char ip[INET_ADDRSTRLEN]; inet_ntop(AF_INET, &d.ip, ip, sizeof ip);
        fprintf(stderr, "ct-egress-bpf: refused private/local dst %s:%u\n", ip, ntohs(d.port));
        close(client); return NULL;
    }
    int up = request_dial(d.ip, d.port);
    if (up < 0) { close(client); return NULL; }
    pthread_t t1, t2;
    struct pump *a = malloc(sizeof *a), *b = malloc(sizeof *b);
    *a = (struct pump){ client, up }; *b = (struct pump){ up, client };
    pthread_create(&t1, NULL, pump, a); pthread_create(&t2, NULL, pump, b);
    pthread_join(t1, NULL); pthread_join(t2, NULL);
    close(up); close(client);
    return NULL;
}

static int enter_netns(int pid) {
    char path[64]; snprintf(path, sizeof path, "/proc/%d/ns/net", pid);
    int fd = open(path, O_RDONLY | O_CLOEXEC);
    if (fd < 0) return -1;
    int rc = setns(fd, CLONE_NEWNET); close(fd); return rc;
}
static int guest_cgroup(char *out, size_t outlen, int pid) {
    char path[64]; snprintf(path, sizeof path, "/proc/%d/cgroup", pid);
    FILE *f = fopen(path, "r"); if (!f) return -1;
    char line[512]; int rc = -1;
    while (fgets(line, sizeof line, f)) {
        char *p = strstr(line, "::");
        if (p) { snprintf(out, outlen, "/sys/fs/cgroup%s", p + 2); out[strcspn(out, "\n")] = 0; rc = 0; break; }
    }
    fclose(f); return rc;
}

int main(int argc, char **argv) {
    if (argc < 2 || argc > 3) {
        fprintf(stderr, "usage: %s <container-init-pid> [upstream-dns-ipv4[:port]]\n", argv[0]);
        return 2;
    }
    int pid = atoi(argv[1]);
    if (argc == 3) {
        char *colon = strchr(argv[2], ':');
        if (colon) { size_t l = (size_t)(colon - argv[2]); if (l >= sizeof upstream_dns) return 2; memcpy(upstream_dns, argv[2], l); upstream_dns[l] = 0; upstream_port = atoi(colon + 1); }
        else { if (strlen(argv[2]) >= sizeof upstream_dns) return 2; strcpy(upstream_dns, argv[2]); }
    }
    signal(SIGPIPE, SIG_IGN);

    int dns_pair[2], egress_pair[2];
    if (socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, dns_pair) < 0 ||
        socketpair(AF_UNIX, SOCK_DGRAM | SOCK_CLOEXEC, 0, egress_pair) < 0) { perror("socketpair"); return 1; }

    pid_t child = fork();
    if (child < 0) { perror("fork"); return 1; }
    if (child > 0) {
        /* host-netns helper: dials and resolves from the host namespace */
        close(dns_pair[1]); close(egress_pair[1]);
        dns_chan = dns_pair[0]; egress_chan = egress_pair[0];
        pthread_t dt, et;
        pthread_create(&dt, NULL, host_dns_loop, NULL);
        pthread_create(&et, NULL, host_egress_loop, NULL);
        int status; waitpid(child, &status, 0);
        fprintf(stderr, "ct-egress-bpf: guest server exited, helper stopping\n");
        return 0;
    }

    /* guest-netns server */
    close(dns_pair[0]); close(egress_pair[0]);
    guest_dns_chan = dns_pair[1]; guest_egress_chan = egress_pair[1];

    char cgroup[512];
    if (guest_cgroup(cgroup, sizeof cgroup, pid) < 0) { fprintf(stderr, "ct-egress-bpf: no cgroup for pid %d\n", pid); return 1; }
    struct bpf_object *obj = bpf_object__open_file("/usr/local/lib/op-dbus/ct_egress.bpf.o", NULL);
    if (!obj) { fprintf(stderr, "ct-egress-bpf: open bpf object failed\n"); return 1; }
    if (bpf_object__load(obj)) { fprintf(stderr, "ct-egress-bpf: load bpf: %s\n", strerror(errno)); return 1; }
    int cg = open(cgroup, O_DIRECTORY | O_RDONLY);
    if (cg < 0) { fprintf(stderr, "ct-egress-bpf: open cgroup %s: %s\n", cgroup, strerror(errno)); return 1; }
    struct bpf_program *c4 = bpf_object__find_program_by_name(obj, "cg_connect4");
    struct bpf_program *so = bpf_object__find_program_by_name(obj, "cg_sockops");
    if (!c4 || !so ||
        bpf_prog_attach(bpf_program__fd(c4), cg, BPF_CGROUP_INET4_CONNECT, BPF_F_ALLOW_MULTI) ||
        bpf_prog_attach(bpf_program__fd(so), cg, BPF_CGROUP_SOCK_OPS, BPF_F_ALLOW_MULTI)) {
        fprintf(stderr, "ct-egress-bpf: attach: %s\n", strerror(errno)); return 1;
    }
    srcport_map_fd = bpf_object__find_map_fd_by_name(obj, "orig_by_srcport");
    if (srcport_map_fd < 0) { fprintf(stderr, "ct-egress-bpf: map missing\n"); return 1; }

    if (enter_netns(pid)) { fprintf(stderr, "ct-egress-bpf: setns: %s\n", strerror(errno)); return 1; }

    /* egress listener on guest loopback */
    int lsn = socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0);
    int one = 1; setsockopt(lsn, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
    struct sockaddr_in a = { .sin_family = AF_INET, .sin_port = htons(PROXY_PORT) };
    a.sin_addr.s_addr = htonl(0x7f000001);
    if (bind(lsn, (void *)&a, sizeof a) || listen(lsn, 128)) { fprintf(stderr, "ct-egress-bpf: listen egress: %s\n", strerror(errno)); return 1; }

    /* DNS stub on 127.0.0.2:53 (UDP + TCP) */
    dns_udp_fd = socket(AF_INET, SOCK_DGRAM | SOCK_CLOEXEC, 0);
    struct sockaddr_in da = { .sin_family = AF_INET, .sin_port = htons(53) };
    inet_pton(AF_INET, DNS_ADDR, &da.sin_addr);
    if (bind(dns_udp_fd, (void *)&da, sizeof da)) { fprintf(stderr, "ct-egress-bpf: bind dns udp: %s\n", strerror(errno)); return 1; }
    int dns_tcp = socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0);
    setsockopt(dns_tcp, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
    if (bind(dns_tcp, (void *)&da, sizeof da) || listen(dns_tcp, 64)) { fprintf(stderr, "ct-egress-bpf: bind dns tcp: %s\n", strerror(errno)); return 1; }

    pthread_t t;
    pthread_create(&t, NULL, dns_response_loop, NULL); pthread_detach(t);
    pthread_create(&t, NULL, egress_response_loop, NULL); pthread_detach(t);
    pthread_create(&t, NULL, dns_udp_loop, NULL); pthread_detach(t);
    pthread_create(&t, NULL, dns_tcp_loop, (void *)(intptr_t)dns_tcp); pthread_detach(t);

    fprintf(stderr, "ct-egress-bpf: pid %d cgroup %s, direct egress 127.0.0.1:%d + DNS %s:53 in guest netns (upstream %s:%d, no proxy)\n",
            pid, cgroup, PROXY_PORT, DNS_ADDR, upstream_dns, upstream_port);
    for (;;) {
        int client = accept4(lsn, NULL, NULL, SOCK_CLOEXEC);
        if (client < 0) { if (errno == EINTR) continue; break; }
        struct conn *c = malloc(sizeof *c); if (!c) { close(client); continue; }
        c->client = client;
        pthread_t th; if (pthread_create(&th, NULL, handle_conn, c) == 0) pthread_detach(th); else { close(client); free(c); }
    }
    return 0;
}
