#!/usr/bin/env python3
"""Detect wait_dep cycles in a runit service tree before you reboot into one.

A cycle here is silent: every service in the ring sits in its wait_dep loop
until the timeout, so nothing logs an error until 90-120s in and the boot has
already failed. Run this after any deploy that touches run scripts.

  ./check-dep-cycles.py                       # live tree, enabled services only
  ./check-dep-cycles.py deploy/runit          # a source tree
  ./check-dep-cycles.py --merge deploy/runit  # live, with the source tree
                                              # overriding it: what you would
                                              # get if you deployed right now
"""
from __future__ import annotations

import os
import re
import sys

LIVE = "/etc/runit/sv"
ENABLED = "/etc/runit/runsvdir/default"

WAIT_DEP = re.compile(r"^\s*wait_dep\s+([A-Za-z0-9_.-]+)", re.M)


def deps(run_script: str) -> list[str] | None:
    try:
        with open(run_script) as handle:
            text = handle.read()
    except OSError:
        return None
    # Skip interpolated names such as `wait_dep $dep`; they are not statically known.
    return [name for name in WAIT_DEP.findall(text) if not name.startswith("$")]


def build(roots: list[str], services: list[str]) -> dict[str, list[str]]:
    graph = {}
    for service in services:
        for root in roots:
            found = deps(os.path.join(root, service, "run"))
            if found is not None:
                graph[service] = found
                break
    return graph


def find_cycles(graph: dict[str, list[str]]) -> list[list[str]]:
    WHITE, GRAY, BLACK = 0, 1, 2
    color = dict.fromkeys(graph, WHITE)
    stack: list[str] = []
    cycles: list[list[str]] = []

    def visit(node: str) -> None:
        color[node] = GRAY
        stack.append(node)
        for neighbour in graph.get(node, []):
            if neighbour not in graph:
                continue
            if color[neighbour] == GRAY:
                cycles.append(stack[stack.index(neighbour):] + [neighbour])
            elif color[neighbour] == WHITE:
                visit(neighbour)
        color[node] = BLACK
        stack.pop()

    for node in list(graph):
        if color[node] == WHITE:
            visit(node)
    return cycles


def main(argv: list[str]) -> int:
    merge = "--merge" in argv
    positional = [argument for argument in argv[1:] if not argument.startswith("-")]

    if merge:
        if not positional:
            print("--merge needs a source tree", file=sys.stderr)
            return 2
        roots = [positional[0], LIVE]
        services = sorted(os.listdir(ENABLED))
    elif positional:
        roots = [positional[0]]
        services = sorted(os.listdir(positional[0]))
    else:
        roots = [LIVE]
        services = sorted(os.listdir(ENABLED))

    graph = build(roots, services)
    cycles = find_cycles(graph)

    print(f"{len(graph)} services parsed from {' + '.join(roots)}")

    dangling = sorted({d for node in graph for d in graph[node] if d not in graph})
    if dangling:
        print("depends on services not in this tree: " + " ".join(dangling))

    if not cycles:
        print("no cycles")
        return 0

    seen = set()
    for cycle in cycles:
        key = frozenset(cycle)
        if key in seen:
            continue
        seen.add(key)
        print("CYCLE: " + " -> ".join(cycle))
    return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
