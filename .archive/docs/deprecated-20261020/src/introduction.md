# Introduction

OP-DBUS is a native, deterministic control plane for Artix Linux infrastructure.
It uses s6 supervision instead of systemd, D-Bus as the only control plane, and
a schema-driven plugin architecture so that every read, write, and tool call
flows through a single source of truth.

This book covers how to build, run, operate, and extend the system. It is
intended for contributors, operators, and agents working in the repository.

For the authoritative agent guidance, see `CLAUDE.md` in the repository root. Where this book and the running tree disagree, the tree wins.
