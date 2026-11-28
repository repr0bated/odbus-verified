"""Provision one verified identity's browser route. Host root only; never reclaim ports."""
import fcntl
import ipaddress
import os
from pathlib import Path
import socket
import sys
import tempfile
import uuid


def publish(path, text, mode):
    fd, tmp = tempfile.mkstemp(dir=path.parent)
    try:
        with os.fdopen(fd, 'w') as f:
            f.write(text + '\n')
            os.fchmod(f.fileno(), mode)
            f.flush()
            os.fsync(f.fileno())
        os.replace(tmp, path)
        directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        Path(tmp).unlink(missing_ok=True)


def assign(session, target, volume):
    if os.geteuid() != 0 or str(uuid.UUID(session)) != session:
        raise ValueError('host root and a canonical session id required')
    sled = Path(volume).resolve(strict=True)
    allowed = [Path('/var/lib', runtime, 'storage-pools', pool, 'custom', 'default_identity-' + session)
               for runtime, pool in [('incus', '3tched-pool'), ('lxd', 'lxd-pool')]]
    if sled not in allowed or (sled / 'session_id').read_text().strip() != session:
        raise ValueError('unexpected host identity volume')
    host, port = target.rsplit(':', 1)
    addr = ipaddress.ip_address(host.strip('[]'))
    if not 0 < int(port) < 65536 or not (addr.is_loopback or str(addr) == '10.0.0.2'):
        raise ValueError('expected a guest-local UI listener')
    target = f'[{addr}]:{int(port)}' if addr.version == 6 else f'{addr}:{int(port)}'
    directory = Path('/var/lib/op-dbus/browser-ports')
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    if directory.is_symlink() or directory.stat().st_uid or directory.stat().st_mode & 0o022:
        raise ValueError('unsafe browser assignment directory')
    with (directory / '.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        assigned = {}
        for path in directory.iterdir():
            if path.name.startswith('.') or str(uuid.UUID(path.name)) != path.name:
                continue
            value = int(path.read_text())
            if not 20000 <= value < 30000 or value in assigned:
                raise ValueError('invalid or duplicate existing browser port')
            assigned[value] = path.name
        publish(sled / 'OP_UI_LISTEN', target, 0o644)
        record = directory / session
        if record.exists():
            port = int(record.read_text())
            publish(sled / 'browser_port', str(port), 0o600)
            return port
        for port in range(20000, 30000):
            if port in assigned:
                continue
            with socket.socket() as probe:
                try:
                    probe.bind(('10.0.0.2', port))
                except OSError as error:
                    import errno
                    if error.errno == errno.EADDRINUSE:
                        continue
                    raise
                publish(record, str(port), 0o600)
                publish(sled / 'browser_port', str(port), 0o600)
                return port
        raise RuntimeError('no free browser port')


if __name__ == '__main__':
    print(assign(*sys.argv[1:]))
