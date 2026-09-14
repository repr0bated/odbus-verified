#!/usr/bin/env python3
"""Publish mail CT services the NetMaker way: loopback Incus + UDS + pub0 TCP.

Expects Incus proxies to expose CT ports on 127.0.0.1 only (not pub0).
For each port: /run/ghostbridge/<sock> and TCP PUB_ADDR:<port> → 127.0.0.1:<port>.
"""
from __future__ import annotations

import asyncio
import os
import sys
from pathlib import Path

# Comma-separated bind addresses (pub0 + mesh host).
BINDS = [
    a.strip()
    for a in os.environ.get("MAIL_BIND_ADDRS", "188.68.58.237,100.69.0.1").split(",")
    if a.strip()
]
GB = Path(os.environ.get("GHOSTBRIDGE_DIR", "/run/ghostbridge"))

# (tcp_port, sock_name)
PORTS = (
    (25, "mail-smtp.sock"),
    (143, "mail-imap.sock"),
    (465, "mail-smtps.sock"),
    (587, "mail-submission.sock"),
    (993, "mail-imaps.sock"),
)


async def pipe(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
    try:
        while True:
            data = await reader.read(65536)
            if not data:
                break
            writer.write(data)
            await writer.drain()
    except Exception:
        pass
    finally:
        try:
            writer.close()
            await writer.wait_closed()
        except Exception:
            pass


async def handle(client_r, client_w, host: str, port: int) -> None:
    try:
        remote_r, remote_w = await asyncio.open_connection(host, port)
    except Exception as exc:
        print(f"mail-port-fabric: connect {host}:{port} failed: {exc}", flush=True)
        client_w.close()
        return
    await asyncio.gather(pipe(client_r, remote_w), pipe(remote_r, client_w))


async def serve_unix(path: Path, port: int) -> asyncio.Server:
    if path.exists():
        path.unlink()

    async def _accept(reader, writer):
        await handle(reader, writer, "127.0.0.1", port)

    server = await asyncio.start_unix_server(_accept, path=str(path))
    os.chmod(path, 0o666)
    print(f"mail-port-fabric: unix {path} -> 127.0.0.1:{port}", flush=True)
    return server


async def serve_tcp(bind: str, port: int) -> asyncio.Server:
    async def _accept(reader, writer):
        await handle(reader, writer, "127.0.0.1", port)

    server = await asyncio.start_server(_accept, host=bind, port=port, reuse_address=True)
    print(f"mail-port-fabric: tcp {bind}:{port} -> 127.0.0.1:{port}", flush=True)
    return server


async def main() -> None:
    if not BINDS:
        print("mail-port-fabric: MAIL_BIND_ADDRS empty", flush=True)
        sys.exit(1)
    GB.mkdir(parents=True, exist_ok=True)
    servers: list[asyncio.Server] = []
    for port, sock_name in PORTS:
        servers.append(await serve_unix(GB / sock_name, port))
        for bind in BINDS:
            servers.append(await serve_tcp(bind, port))
    await asyncio.gather(*(s.serve_forever() for s in servers))


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        sys.exit(0)
