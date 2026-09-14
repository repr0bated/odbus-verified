#!/usr/bin/env python3
"""TLS-terminating TCP relay.

Terminates TLS on LISTEN and forwards the plaintext to BACKEND. Modelled on
nm-api-tls.py but generic, so one script can front any loopback service
without the service learning TLS itself.

Env:
  LISTEN_HOST   default 0.0.0.0
  LISTEN_PORT   default 8448
  BACKEND_HOST  default 127.0.0.1
  BACKEND_PORT  default 8080
  CERT          cert chain path
  KEY           private key path
"""
import asyncio
import os
import ssl

LISTEN_HOST = os.environ.get("LISTEN_HOST", "0.0.0.0")
LISTEN_PORT = int(os.environ.get("LISTEN_PORT", "8448"))
BACKEND_HOST = os.environ.get("BACKEND_HOST", "127.0.0.1")
BACKEND_PORT = int(os.environ.get("BACKEND_PORT", "8080"))
CERT = os.environ.get("CERT", "/etc/op-dbus/tls/op-web-mesh.crt")
KEY = os.environ.get("KEY", "/etc/op-dbus/tls/op-web-mesh.key")


async def pipe(reader, writer):
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


async def handle(reader, writer):
    try:
        br, bw = await asyncio.open_connection(BACKEND_HOST, BACKEND_PORT)
    except Exception as e:
        print("backend connect failed", e, flush=True)
        writer.close()
        return
    await asyncio.gather(pipe(reader, bw), pipe(br, writer))


async def main():
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.minimum_version = ssl.TLSVersion.TLSv1_2
    ctx.load_cert_chain(CERT, KEY)
    server = await asyncio.start_server(handle, LISTEN_HOST, LISTEN_PORT, ssl=ctx)
    print(f"tls-relay: https://{LISTEN_HOST}:{LISTEN_PORT} -> {BACKEND_HOST}:{BACKEND_PORT}", flush=True)
    async with server:
        await server.serve_forever()


if __name__ == "__main__":
    asyncio.run(main())
