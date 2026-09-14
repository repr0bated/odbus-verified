#!/usr/bin/env python3
"""TLS ClientHello SNI demux in front of Reality / backend UDS.

Usage:
  sni-demux.py \\
    --listen 188.68.58.237:443 --listen 10.0.0.2:443 \\
    --map mail.3tched.com=/run/ghostbridge/mail-web.sock \\
    --map mail.ghostbridge.tech=/run/ghostbridge/mail-web.sock \\
    --default /run/ghostbridge/xray-reality.sock
"""
from __future__ import annotations

import argparse
import asyncio
import sys
from pathlib import Path

TLS_HEADER_LEN = 5
MAX_TLS_RECORD_LEN = 18_432
MAX_CLIENT_HELLO_LEN = 65_536


def _handshake_payload(data: bytes) -> bytes:
    """Join complete TLS handshake records from an initial byte sequence."""
    payload = bytearray()
    offset = 0
    while offset + TLS_HEADER_LEN <= len(data):
        if data[offset] != 0x16:
            break
        rec_len = int.from_bytes(data[offset + 3 : offset + 5], "big")
        record_end = offset + TLS_HEADER_LEN + rec_len
        if record_end > len(data):
            break
        payload.extend(data[offset + TLS_HEADER_LEN : record_end])
        offset = record_end
    return bytes(payload)


def extract_sni(data: bytes) -> str | None:
    """Best-effort SNI extraction from one or more TLS ClientHello records."""
    if len(data) < TLS_HEADER_LEN or data[0] != 0x16:
        return None
    hello = _handshake_payload(data)
    if len(hello) < 42 or hello[0] != 0x01:  # handshake type client_hello
        return None
    hello_len = int.from_bytes(hello[1:4], "big")
    if hello_len > MAX_CLIENT_HELLO_LEN or len(hello) < 4 + hello_len:
        return None
    hello = hello[: 4 + hello_len]
    # Handshake header: type(1) len(3) + client_version(2) random(32)
    body = hello[4:]
    if len(body) < 34:
        return None
    idx = 34
    # session_id
    if idx >= len(body):
        return None
    sid_len = body[idx]
    idx += 1 + sid_len
    # cipher_suites
    if idx + 2 > len(body):
        return None
    cs_len = int.from_bytes(body[idx : idx + 2], "big")
    idx += 2 + cs_len
    # compression
    if idx + 1 > len(body):
        return None
    comp_len = body[idx]
    idx += 1 + comp_len
    # extensions
    if idx + 2 > len(body):
        return None
    ext_len = int.from_bytes(body[idx : idx + 2], "big")
    idx += 2
    end = min(len(body), idx + ext_len)
    while idx + 4 <= end:
        etype = int.from_bytes(body[idx : idx + 2], "big")
        elen = int.from_bytes(body[idx + 2 : idx + 4], "big")
        idx += 4
        if idx + elen > end:
            break
        if etype == 0x0000 and elen >= 5:  # server_name
            # list_len(2) name_type(1) name_len(2) name
            nlist = body[idx : idx + elen]
            if len(nlist) < 5:
                break
            # skip list length
            nidx = 2
            if nlist[nidx] != 0:  # host_name
                break
            nlen = int.from_bytes(nlist[nidx + 1 : nidx + 3], "big")
            nidx += 3
            if nidx + nlen > len(nlist):
                break
            try:
                return nlist[nidx : nidx + nlen].decode("ascii")
            except UnicodeDecodeError:
                return None
        idx += elen
    return None


async def read_client_hello(reader: asyncio.StreamReader) -> bytes:
    """Read complete TLS records until the initial ClientHello is complete.

    StreamReader.read() may return a partial TCP segment. Reading an arbitrary
    2 KiB chunk therefore misroutes valid fragmented or post-quantum-sized
    ClientHello messages to the default backend.
    """
    raw = bytearray()
    handshake_len: int | None = None

    while True:
        header = await reader.readexactly(TLS_HEADER_LEN)
        raw.extend(header)
        if header[0] != 0x16:
            return bytes(raw)

        rec_len = int.from_bytes(header[3:5], "big")
        if rec_len > MAX_TLS_RECORD_LEN:
            return bytes(raw)

        record = await reader.readexactly(rec_len)
        raw.extend(record)

        payload = _handshake_payload(bytes(raw))
        if len(payload) >= 4:
            if payload[0] != 0x01:
                return bytes(raw)
            handshake_len = int.from_bytes(payload[1:4], "big")
            if handshake_len > MAX_CLIENT_HELLO_LEN:
                return bytes(raw)
        if handshake_len is not None and len(payload) >= 4 + handshake_len:
            return bytes(raw)


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


async def handle(
    client_r: asyncio.StreamReader,
    client_w: asyncio.StreamWriter,
    routes: dict[str, str],
    default_sock: str,
) -> None:
    peer = client_w.get_extra_info("peername")
    try:
        first = await asyncio.wait_for(read_client_hello(client_r), timeout=5)
    except asyncio.IncompleteReadError as exc:
        # Preserve a short non-TLS probe for the default backend when possible.
        first = exc.partial
    except Exception as exc:
        print(f"sni-demux: peek failed from {peer}: {exc}", flush=True)
        client_w.close()
        return
    if not first:
        client_w.close()
        return

    sni = extract_sni(first) or ""
    dest = routes.get(sni.lower(), default_sock)
    print(f"sni-demux: {peer} sni={sni or '-'} -> {dest}", flush=True)

    try:
        remote_r, remote_w = await asyncio.open_unix_connection(dest)
    except Exception as exc:
        print(f"sni-demux: connect {dest} failed: {exc}", flush=True)
        client_w.close()
        return

    remote_w.write(first)
    await remote_w.drain()
    await asyncio.gather(pipe(client_r, remote_w), pipe(remote_r, client_w))


async def main_async(args: argparse.Namespace) -> None:
    routes = {}
    for item in args.map:
        if "=" not in item:
            raise SystemExit(f"bad --map {item!r}, expected name=path")
        name, path = item.split("=", 1)
        routes[name.strip().lower()] = path.strip()

    default_sock = args.default
    for p in list(routes.values()) + [default_sock]:
        # Wait briefly for socks at startup; keep going if default arrives later
        for _ in range(60):
            if Path(p).is_socket():
                break
            await asyncio.sleep(1)

    servers = []

    async def _accept(reader, writer):
        await handle(reader, writer, routes, default_sock)

    for listen in args.listen:
        host, port_s = listen.rsplit(":", 1)
        port = int(port_s)
        srv = await asyncio.start_server(_accept, host=host, port=port, reuse_address=True)
        servers.append(srv)
        print(f"sni-demux: listen {host}:{port}", flush=True)

    await asyncio.gather(*(s.serve_forever() for s in servers))


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--listen", action="append", required=True, help="host:port")
    ap.add_argument("--map", action="append", default=[], help="sni=/path/to.sock")
    ap.add_argument("--default", required=True, help="default unix socket path")
    args = ap.parse_args(argv[1:])
    try:
        asyncio.run(main_async(args))
    except KeyboardInterrupt:
        return 0
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
