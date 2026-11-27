// SPDX-License-Identifier: Apache-2.0
#define _GNU_SOURCE
#include <bpf/libbpf.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static int protected_dir(const char *path)
{
    struct stat st;
    if (mkdir(path, 0700) < 0 && errno != EEXIST)
        return -1;
    if (lstat(path, &st) < 0 || !S_ISDIR(st.st_mode) || st.st_uid != 0 || (st.st_mode & 0077))
        return -1;
    return 0;
}

int main(int argc, char **argv)
{
    struct bpf_object *object = NULL;
    struct bpf_program *program;
    struct bpf_map *routes;
    struct bpf_link *link = NULL;
    char map_path[PATH_MAX], link_path[PATH_MAX];
    int cgroup_fd = -1, error = 1;

    if (argc != 4) {
        fprintf(stderr, "usage: browser-ingress-bpf OBJECT CGROUP PIN_DIR\n");
        return 2;
    }
    if (geteuid() != 0 || protected_dir(argv[3]) < 0) {
        fprintf(stderr, "browser ingress requires a root-owned mode-0700 pin directory\n");
        return 1;
    }
    if (snprintf(map_path, sizeof map_path, "%s/browser_routes", argv[3]) >= (int)sizeof map_path ||
        snprintf(link_path, sizeof link_path, "%s/browser_ingress_link", argv[3]) >= (int)sizeof link_path) {
        fprintf(stderr, "pin path is too long\n");
        return 1;
    }
    cgroup_fd = open(argv[2], O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW);
    if (cgroup_fd < 0) {
        perror("open cgroup");
        goto out;
    }
    object = bpf_object__open_file(argv[1], NULL);
    if (!object || libbpf_get_error(object)) {
        object = NULL;
        fprintf(stderr, "open browser ingress object failed\n");
        goto out;
    }
    if (bpf_object__load(object)) {
        fprintf(stderr, "load browser ingress object failed\n");
        goto out;
    }
    routes = bpf_object__find_map_by_name(object, "browser_routes");
    program = bpf_object__find_program_by_name(object, "browser_ingress");
    if (!routes || !program) {
        fprintf(stderr, "browser ingress object is incomplete\n");
        goto out;
    }
    unlink(map_path);
    unlink(link_path);
    if (bpf_map__pin(routes, map_path)) {
        fprintf(stderr, "pin browser route map failed\n");
        goto out;
    }
    link = bpf_program__attach_cgroup(program, cgroup_fd);
    if (!link || libbpf_get_error(link)) {
        link = NULL;
        unlink(map_path);
        fprintf(stderr, "attach browser ingress program failed\n");
        goto out;
    }
    if (bpf_link__pin(link, link_path)) {
        unlink(map_path);
        fprintf(stderr, "pin browser ingress link failed\n");
        goto out;
    }
    chmod(map_path, 0600);
    chmod(link_path, 0600);
    printf("browser ingress attached; capacity=4096 port=8080\n");
    error = 0;
out:
    if (link)
        bpf_link__destroy(link);
    if (object)
        bpf_object__close(object);
    if (cgroup_fd >= 0)
        close(cgroup_fd);
    return error;
}
