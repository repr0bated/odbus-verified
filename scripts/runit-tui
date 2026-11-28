#!/usr/bin/env python3
"""
runit-tui — operator TUI for Artix/runit host services.

Auto-detects new/removed services by rescanning SVDIR.
Reads status from supervise/stat+pid when possible (no root required for view).
Control actions (up/down/restart) try `sv`, then `sudo -n sv`.

Keys:
  ↑/↓ j/k     select service
  d           details: description, paths, flags, conf, run script, options
  Enter/l     focus log pane
  Tab         toggle list/log
  r           restart
  u / s       up (start)
  t / x       down (stop)   — NOT "d"
  o           once
  c           check
  f           force rescan (pick up new services)
  /           filter
  ? / h       help overlay
  q           quit

Usage:
  runit-tui.py
  sudo runit-tui.py          # full control without sudo prompts
  SVDIR=/run/runit/service runit-tui.py --rescan 1
"""

from __future__ import annotations

import argparse
import curses
import os
import re
import stat as statmod
import subprocess
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import List, Optional, Sequence, Tuple

DEFAULT_SVDIRS = (
    "/run/runit/service",
    "/etc/runit/runsvdir/current",
    "/etc/runit/runsvdir/default",
)
LOG_ROOTS = (
    Path("/var/log/op-dbus"),
    Path("/var/log"),
)
RESCAN_INTERVAL_S = 2.0
STATUS_REFRESH_S = 1.0
LOG_REFRESH_S = 0.8
MAX_LOG_LINES = 400
MAX_DETAIL_LINES = 500

# Human descriptions for known 3tched services (extend as needed)
SERVICE_BLURB = {
    "op-web": "HTTP API + dashboard (Axum). Static UI from OP_WEB_STATIC_DIR.",
    "op-grpc-bridge": "Unified TLS :8090 MCP/gRPC door plus local grpc.sock.",
    "op-session-bus": "SESSION D-Bus at /run/opdbus/session-bus.sock.",
    "opdbus-rundirs": "Creates /run/opdbus, SHM catalog, seeds identity sled.",
    "op-of-controller": "OpenFlow controller 127.0.0.1:6653.",
    "ovsdb-server": "OVS database server.",
    "ovs-vswitchd": "OVS datapath daemon.",
    "ovsbr0-uplink": "Bring up ovsbr0 + enslave uplink.",
    "ovsbr0-addr": "pub0/svc0 L3 addresses + default route.",
    "ovsbr0-svc-addr": "Reaffirm service addresses; strip mesh-on-bridge mistakes.",
    "uplink-dhcp": "Snapshot uplink into /run/opdbus/uplink-migration.env.",
    "wg-3tched": "WireGuard mesh interface 3tched (100.69.0.254/16).",
    "xray-config-mount": "Bind /dev/shm/xray_config.json into xray CT (SHM only).",
    "incusd": "Incus container daemon.",
    "fwd-8444": "Mesh 100.69.0.254:8444 → xray VLESS.",
    "uds-assistant": "UDS forward for assistant controlplane http.sock.",
    "notebook-sources-sync": "Periodic LLM session → notebook sources sync.",
    "dbus": "System D-Bus.",
    "sshd": "OpenSSH daemon.",
}


def set_rescan_interval(seconds: float) -> None:
    global RESCAN_INTERVAL_S
    RESCAN_INTERVAL_S = max(0.5, float(seconds))


def resolve_svdir(explicit: Optional[str] = None) -> Path:
    if explicit:
        p = Path(explicit)
        if not p.is_dir():
            raise SystemExit(f"SVDIR not a directory: {p}")
        return p
    env = os.environ.get("SVDIR")
    if env and Path(env).is_dir():
        return Path(env)
    for cand in DEFAULT_SVDIRS:
        p = Path(cand)
        if p.is_dir():
            try:
                if any(p.iterdir()):
                    return p
            except OSError:
                continue
    for cand in DEFAULT_SVDIRS:
        if Path(cand).is_dir():
            return Path(cand)
    raise SystemExit(
        "No runit service directory found. Set SVDIR= or pass --svdir "
        "(tried: " + ", ".join(DEFAULT_SVDIRS) + ")"
    )


def is_root() -> bool:
    return os.geteuid() == 0


@dataclass
class Service:
    name: str
    path: Path
    status_raw: str = ""
    state: str = "?"  # run | down | finish | fail | ?
    pid: str = ""
    seconds: str = ""
    log_path: Optional[Path] = None
    has_log_dir: bool = False
    access: str = "ok"  # ok | denied | missing


@dataclass
class AppState:
    svdir: Path
    services: List[Service] = field(default_factory=list)
    filtered: List[Service] = field(default_factory=list)
    selected: int = 0
    filter: str = ""
    focus: str = "list"  # list | log | filter | detail | help
    log_lines: List[str] = field(default_factory=list)
    log_scroll: int = 0
    detail_lines: List[str] = field(default_factory=list)
    detail_scroll: int = 0
    message: str = ""
    message_until: float = 0.0
    last_scan_names: Tuple[str, ...] = ()
    last_scan_at: float = 0.0
    last_status_at: float = 0.0
    last_log_at: float = 0.0
    filter_buf: str = ""
    new_flash: set = field(default_factory=set)
    flash_until: float = 0.0
    elevated: bool = False  # True if we can run sv without denial


def _sv_cmd(args: Sequence[str], env: dict, timeout: float = 3.0) -> Tuple[int, str]:
    """Run sv; if access denied and not root, retry with sudo -n."""
    try:
        r = subprocess.run(
            ["sv", *args],
            capture_output=True,
            text=True,
            timeout=timeout,
            env=env,
        )
        out = ((r.stdout or "") + (r.stderr or "")).strip()
        denied = "access denied" in out.lower() or "permission denied" in out.lower()
        if denied and not is_root():
            r2 = subprocess.run(
                ["sudo", "-n", "sv", *args],
                capture_output=True,
                text=True,
                timeout=timeout,
                env=env,
            )
            out2 = ((r2.stdout or "") + (r2.stderr or "")).strip()
            if r2.returncode == 0 or (
                out2 and "access denied" not in out2.lower()
            ):
                return r2.returncode, out2
            if "password" in out2.lower() or r2.returncode == 1:
                return r.returncode, out + " | sudo -n failed (run TUI as root: sudo runit-tui)"
            return r2.returncode, out2 or out
        return r.returncode, out
    except FileNotFoundError:
        return 127, "sv: command not found"
    except subprocess.TimeoutExpired:
        return 124, "sv: timed out"


def supervise_dir(svc_path: Path) -> Optional[Path]:
    """Locate supervise/ even when directory listing is mode 0700."""
    # Common layouts: SVDIR/name -> /etc/runit/sv/name
    candidates = [
        svc_path / "supervise",
        svc_path.resolve() / "supervise" if svc_path.is_symlink() else None,
    ]
    # Also try canonical /etc/runit/sv/<name>
    name = svc_path.name
    candidates.append(Path("/etc/runit/sv") / name / "supervise")
    candidates.append(Path("/run/runit/service") / name / "supervise")
    for c in candidates:
        if c is None:
            continue
        # stat files may be world-readable even if dir is 0700
        for leaf in ("stat", "pid", "status"):
            try:
                p = c / leaf
                if p.is_file():
                    return c
            except OSError:
                continue
    return None


def read_status_from_supervise(svc_path: Path) -> Tuple[str, str, str, str, str]:
    """
    Read run/down without needing open(supervise/ok).
    Returns (state, pid, seconds, raw, access).
    """
    sup = supervise_dir(svc_path)
    if not sup:
        return "?", "", "", "supervise not found", "missing"

    state = "?"
    pid = ""
    raw_parts = []
    access = "ok"

    # stat is typically world-readable: "run\n" or "down\n"
    try:
        st = (sup / "stat").read_text(errors="replace").strip()
        raw_parts.append(f"stat={st}")
        if st in ("run", "down", "finish"):
            state = st
    except PermissionError:
        access = "denied"
        raw_parts.append("stat=permission denied")
    except OSError as e:
        raw_parts.append(f"stat={e}")

    try:
        pid = (sup / "pid").read_text(errors="replace").strip()
        if pid and pid != "0":
            raw_parts.append(f"pid={pid}")
        else:
            pid = ""
    except PermissionError:
        access = "denied"
    except OSError:
        pass

    # Binary status has timestamp for uptime; optional best-effort
    seconds = ""
    try:
        blob = (sup / "status").read_bytes()
        # runit status: 20 bytes; TAI64N-ish timestamp in first 8-12 — skip complex parse
        if len(blob) >= 20 and state == "run" and pid:
            # approximate: use /proc/pid starttime if available
            try:
                with open(f"/proc/{pid}/stat") as f:
                    fields = f.read().split()
                # field 21 (0-index 21) starttime in clock ticks
                start_ticks = int(fields[21])
                hz = os.sysconf(os.sysconf_names.get("SC_CLK_TCK", "SC_CLK_TCK"))
                uptime = float(Path("/proc/uptime").read_text().split()[0])
                # process start ≈ uptime - (start_ticks/hz) is wrong; use:
                # starttime is ticks since boot
                sec = max(0, int(uptime - start_ticks / hz))
                seconds = str(sec)
            except Exception:
                pass
    except Exception:
        pass

    if state == "run" and pid:
        raw = f"run: {svc_path.name}: (pid {pid})" + (f" {seconds}s" if seconds else "")
        raw += " [supervise files]"
    elif state == "down":
        raw = f"down: {svc_path.name}" + (f" {seconds}s" if seconds else "")
        raw += " [supervise files]"
    else:
        raw = " ".join(raw_parts) or f"{svc_path.name}: unknown"

    if access == "denied" and state == "?":
        raw = f"access denied reading supervise for {svc_path.name}"
    return state, pid, seconds, raw, access


def parse_status(raw: str) -> Tuple[str, str, str]:
    raw = raw.strip()
    if not raw:
        return "?", "", ""
    low = raw.lower()
    if (
        raw.startswith("fail:")
        or "unable to" in low
        or "runsv not running" in low
        or "access denied" in low
    ):
        # Still try to extract if mixed
        if raw.startswith("run:"):
            pass
        else:
            return "fail", "", ""
    if raw.startswith("run:") or raw.startswith("run "):
        m = re.search(r"\(pid\s+(\d+)\)\s*(\d+)?s?", raw)
        if m:
            return "run", m.group(1), m.group(2) or ""
        m2 = re.search(r"pid[= ](\d+)", raw)
        if m2:
            return "run", m2.group(1), ""
        return "run", "", ""
    if raw.startswith("down:"):
        m = re.search(r"(\d+)s", raw)
        return "down", "", m.group(1) if m else ""
    if raw.startswith("finish:"):
        return "finish", "", ""
    return "?", "", ""


def discover_log_path(name: str) -> Tuple[Optional[Path], bool]:
    for base in LOG_ROOTS:
        p = base / name / "current"
        try:
            if p.is_file():
                return p, True
            if (base / name).is_dir():
                return p, True
        except OSError:
            continue
    preferred = LOG_ROOTS[0] / name / "current"
    return preferred, False


def scan_services(svdir: Path) -> List[Service]:
    out: List[Service] = []
    try:
        entries = sorted(svdir.iterdir(), key=lambda p: p.name.lower())
    except OSError as e:
        return [
            Service(
                name=f"<error {svdir}>",
                path=svdir,
                status_raw=str(e),
                state="fail",
            )
        ]

    for ent in entries:
        if not (ent.is_dir() or ent.is_symlink()):
            continue
        try:
            real = ent.resolve() if ent.is_symlink() else ent
            if not real.is_dir():
                continue
        except OSError:
            continue
        name = ent.name
        if name.startswith("."):
            continue
        log_path, has_log = discover_log_path(name)
        out.append(
            Service(name=name, path=ent, log_path=log_path, has_log_dir=has_log)
        )
    return out


def refresh_status(services: List[Service], svdir: Path) -> None:
    env = os.environ.copy()
    env["SVDIR"] = str(svdir)
    for svc in services:
        if svc.name.startswith("<"):
            continue
        # 1) Prefer world-readable supervise files (no root)
        state, pid, secs, raw, access = read_status_from_supervise(svc.path)
        if state in ("run", "down", "finish"):
            svc.state, svc.pid, svc.seconds = state, pid, secs
            svc.status_raw = raw
            svc.access = access
            continue

        # 2) Fall back to sv (may need sudo)
        code, out = _sv_cmd(["status", svc.name], env, timeout=2.0)
        line = out.split("\n")[0] if out else ""
        if "access denied" in line.lower():
            svc.access = "denied"
            # keep supervise partial if any
            if state != "?":
                svc.state, svc.pid, svc.seconds = state, pid, secs
                svc.status_raw = raw
            else:
                svc.state = "fail"
                svc.status_raw = line or "access denied (try: sudo runit-tui)"
            continue
        svc.access = "ok"
        svc.status_raw = line
        st, p, s = parse_status(line)
        svc.state, svc.pid, svc.seconds = st, p, s


def read_log(path: Optional[Path], max_lines: int = MAX_LOG_LINES) -> List[str]:
    if not path:
        return ["(no log path)"]
    try:
        if not path.is_file():
            if path.parent.is_dir():
                return [f"(waiting for log) {path}"]
            return [f"(no log yet) {path}"]
        with open(path, "rb") as f:
            f.seek(0, os.SEEK_END)
            size = f.tell()
            data = b""
            block = 64 * 1024
            while size > 0 and data.count(b"\n") <= max_lines:
                step = min(block, size)
                size -= step
                f.seek(size)
                data = f.read(step) + data
            text = data.decode("utf-8", errors="replace")
        lines = text.splitlines()
        return lines[-max_lines:] if len(lines) > max_lines else lines
    except PermissionError:
        return [
            f"(permission denied reading {path})",
            "Hint: sudo runit-tui   or   chmod/ACLs on log dir",
        ]
    except OSError as e:
        return [f"(log error: {e})"]


def read_text_file(path: Path, limit: int = 80) -> List[str]:
    try:
        text = path.read_text(errors="replace")
        lines = text.splitlines()
        if len(lines) > limit:
            return lines[:limit] + [f"… ({len(lines) - limit} more lines)"]
        return lines
    except PermissionError:
        return [f"(permission denied: {path})"]
    except OSError as e:
        return [f"({e})"]


def build_service_details(svc: Service, svdir: Path) -> List[str]:
    """Description, paths, flags, conf, run script, sv options."""
    lines: List[str] = []
    real = svc.path
    try:
        if svc.path.is_symlink():
            real = svc.path.resolve()
    except OSError:
        pass

    blurb = SERVICE_BLURB.get(
        svc.name,
        "runit longrun service (see run script). No canned blurb for this name.",
    )

    lines.append(f"═══ {svc.name} ═══")
    lines.append("")
    lines.append("DESCRIPTION")
    lines.append(f"  {blurb}")
    lines.append("")
    lines.append("STATUS")
    lines.append(f"  state     : {svc.state}")
    lines.append(f"  pid       : {svc.pid or '—'}")
    lines.append(f"  uptime    : {svc.seconds + 's' if svc.seconds else '—'}")
    lines.append(f"  access    : {svc.access}")
    lines.append(f"  sv status : {svc.status_raw or '—'}")
    lines.append("")
    lines.append("PATHS")
    lines.append(f"  SVDIR entry : {svc.path}")
    lines.append(f"  resolved    : {real}")
    lines.append(f"  log         : {svc.log_path or '—'}")
    sup = supervise_dir(svc.path)
    lines.append(f"  supervise   : {sup or '—'}")
    lines.append("")

    # Flags / features from directory layout
    lines.append("FLAGS / FEATURES")
    flags = []
    for name, label in (
        ("run", "has run script"),
        ("finish", "has finish script"),
        ("check", "has check script"),
        ("down", "down file (starts down)"),
        ("log", "has log/ service"),
        ("conf", "has conf file"),
        ("env", "has env/ dir"),
        ("control", "has control/ hooks"),
    ):
        p = real / name
        try:
            exists = p.exists()
        except OSError:
            exists = False
        flags.append(f"  [{'x' if exists else ' '}] {label}  ({p})")
    lines.extend(flags)
    lines.append("")

    # conf file (environment flags often live here)
    conf = real / "conf"
    if conf.is_file():
        lines.append("CONF / FLAGS FILE (./conf)")
        for ln in read_text_file(conf, 40):
            lines.append(f"  {ln}")
        lines.append("")
    env_dir = real / "env"
    if env_dir.is_dir():
        lines.append("ENV DIR (chpst -e style)")
        try:
            for ent in sorted(env_dir.iterdir()):
                if ent.is_file():
                    try:
                        val = ent.read_text(errors="replace").strip().replace("\n", " ")
                        if len(val) > 80:
                            val = val[:77] + "…"
                        lines.append(f"  {ent.name}={val}")
                    except OSError as e:
                        lines.append(f"  {ent.name}: ({e})")
        except PermissionError:
            lines.append("  (permission denied listing env/)")
        lines.append("")

    # run script
    run_script = real / "run"
    lines.append("RUN SCRIPT")
    if run_script.is_file():
        for ln in read_text_file(run_script, 60):
            lines.append(f"  {ln}")
    else:
        lines.append("  (no run script found)")
    lines.append("")

    log_run = real / "log" / "run"
    if log_run.is_file():
        lines.append("LOG/RUN")
        for ln in read_text_file(log_run, 20):
            lines.append(f"  {ln}")
        lines.append("")

    lines.append("SV OPTIONS (keys in this TUI)")
    lines.append("  u / s     sv up      — start and keep up")
    lines.append("  t / x     sv down    — stop and stay down")
    lines.append("  r         sv restart — down then up")
    lines.append("  o         sv once    — run once (no restart on exit)")
    lines.append("  c         sv check   — wait until up / report")
    lines.append("  d         this details panel")
    lines.append("  f         rescan SVDIR for new services")
    lines.append("")
    lines.append("CLI EQUIVALENTS")
    lines.append(f"  SVDIR={svdir} sv status {svc.name}")
    lines.append(f"  SVDIR={svdir} sv up|down|restart {svc.name}")
    if svc.log_path:
        lines.append(f"  tail -f {svc.log_path}")
    lines.append("")
    if not is_root() and svc.access == "denied":
        lines.append("NOTE: status used supervise files or was denied.")
        lines.append("  Full control:  sudo runit-tui")
        lines.append("  Or passwordless: sudo -n sv status <name>")
    return lines


HELP_TEXT = """
runit-tui help
══════════════
r  = restart (sv restart) — NOT "refresh"
f  = force rescan SVDIR (detect NEW services)
d  = details: description, paths, flags, conf, run script, options
t/x = down/stop   (d is details, not down)
u/s = up/start
o  = once
c  = check
/  = filter list
↑↓ = select · Tab/Enter/l = logs · q = quit

Auto-rescan every few seconds picks up services symlinked into SVDIR.
Viewing status works without root (reads supervise/stat+pid when readable).
Start/stop/restart need root or sudo -n.
""".strip().splitlines()


def apply_filter(state: AppState) -> None:
    q = state.filter.lower().strip()
    if not q:
        state.filtered = list(state.services)
    else:
        state.filtered = [
            s
            for s in state.services
            if q in s.name.lower() or q in s.state.lower()
        ]
    if state.selected >= len(state.filtered):
        state.selected = max(0, len(state.filtered) - 1)
    if state.selected < 0:
        state.selected = 0


def rescan(state: AppState, force_status: bool = True) -> None:
    now = time.time()
    new_list = scan_services(state.svdir)
    new_names = tuple(s.name for s in new_list)
    old_names = set(state.last_scan_names)
    new_set = set(new_names)
    added = new_set - old_names
    removed = old_names - new_set

    old_by_name = {s.name: s for s in state.services}
    merged: List[Service] = []
    for s in new_list:
        if s.name in old_by_name:
            prev = old_by_name[s.name]
            s.status_raw = prev.status_raw
            s.state = prev.state
            s.pid = prev.pid
            s.seconds = prev.seconds
            s.access = prev.access
        merged.append(s)

    state.services = merged
    if added:
        state.new_flash |= added
        state.flash_until = now + 8.0
        state.message = f"+{len(added)} new: {', '.join(sorted(added)[:6])}"
        if len(added) > 6:
            state.message += "…"
        state.message_until = now + 5.0
    elif removed:
        state.message = f"-{len(removed)} removed: {', '.join(sorted(removed)[:6])}"
        state.message_until = now + 5.0

    state.last_scan_names = new_names
    state.last_scan_at = now
    apply_filter(state)
    if force_status:
        refresh_status(state.services, state.svdir)
        state.last_status_at = now


def sv_action(state: AppState, action: str) -> None:
    if not state.filtered:
        return
    svc = state.filtered[state.selected]
    env = os.environ.copy()
    env["SVDIR"] = str(state.svdir)
    code, out = _sv_cmd([action, svc.name], env, timeout=5.0)
    short = out.split("\n")[0] if out else ("ok" if code == 0 else f"rc={code}")
    state.message = f"sv {action} {svc.name}: {short}"
    state.message_until = time.time() + 4.0
    refresh_status(state.services, state.svdir)
    state.last_status_at = time.time()


def selected_service(state: AppState) -> Optional[Service]:
    if not state.filtered:
        return None
    if state.selected < 0 or state.selected >= len(state.filtered):
        return None
    return state.filtered[state.selected]


def refresh_log(state: AppState) -> None:
    svc = selected_service(state)
    if not svc:
        state.log_lines = ["(no service selected)"]
        return
    path, has = discover_log_path(svc.name)
    svc.log_path, svc.has_log_dir = path, has
    state.log_lines = read_log(path)


def open_details(state: AppState) -> None:
    svc = selected_service(state)
    if not svc:
        state.message = "no service selected"
        state.message_until = time.time() + 2
        return
    state.detail_lines = build_service_details(svc, state.svdir)
    state.detail_scroll = 0
    state.focus = "detail"


# ── drawing ──────────────────────────────────────────────────────────────────

STATE_ATTR = {
    "run": 1,
    "down": 2,
    "fail": 3,
    "finish": 4,
    "?": 5,
}


def init_colors() -> None:
    curses.start_color()
    curses.use_default_colors()
    curses.init_pair(1, curses.COLOR_GREEN, -1)
    curses.init_pair(2, curses.COLOR_YELLOW, -1)
    curses.init_pair(3, curses.COLOR_RED, -1)
    curses.init_pair(4, curses.COLOR_CYAN, -1)
    curses.init_pair(5, curses.COLOR_WHITE, -1)
    curses.init_pair(6, curses.COLOR_BLACK, curses.COLOR_CYAN)
    curses.init_pair(7, curses.COLOR_BLACK, curses.COLOR_GREEN)
    curses.init_pair(8, curses.COLOR_MAGENTA, -1)
    curses.init_pair(9, curses.COLOR_BLACK, curses.COLOR_WHITE)
    curses.init_pair(10, curses.COLOR_BLACK, curses.COLOR_BLUE)


def draw_overlay(
    stdscr: "curses._CursesWindow",
    lines: List[str],
    scroll: int,
    title: str,
) -> None:
    h, w = stdscr.getmaxyx()
    margin_y, margin_x = 1, 2
    box_h = h - 2 * margin_y
    box_w = w - 2 * margin_x
    if box_h < 5 or box_w < 20:
        return
    # Clear box
    for y in range(margin_y, margin_y + box_h):
        try:
            stdscr.addstr(y, margin_x, " " * (box_w - 1), curses.color_pair(10))
        except curses.error:
            pass
    try:
        stdscr.addstr(
            margin_y,
            margin_x,
            f" {title}  (Esc/d/q close · ↑↓ scroll) "[: box_w - 1].ljust(box_w - 1),
            curses.color_pair(6) | curses.A_BOLD,
        )
    except curses.error:
        pass
    body_h = box_h - 2
    if scroll < 0:
        scroll = 0
    max_scroll = max(0, len(lines) - body_h)
    if scroll > max_scroll:
        scroll = max_scroll
    view = lines[scroll : scroll + body_h]
    for i, line in enumerate(view):
        try:
            stdscr.addstr(
                margin_y + 1 + i,
                margin_x + 1,
                line[: box_w - 3],
                curses.color_pair(10),
            )
        except curses.error:
            pass


def draw(stdscr: "curses._CursesWindow", state: AppState) -> None:
    stdscr.erase()
    h, w = stdscr.getmaxyx()
    if h < 8 or w < 40:
        stdscr.addstr(0, 0, "terminal too small")
        stdscr.refresh()
        return

    now = time.time()
    if now > state.flash_until:
        state.new_flash.clear()

    who = "root" if is_root() else "user"
    title = (
        f" runit-tui  SVDIR={state.svdir}  n={len(state.services)}  "
        f"as={who}  rescan={RESCAN_INTERVAL_S:.0f}s "
    )
    if state.filter:
        title += f" /{state.filter} "
    stdscr.attron(curses.color_pair(8) | curses.A_BOLD)
    stdscr.addstr(0, 0, title[: w - 1].ljust(w - 1))
    stdscr.attroff(curses.color_pair(8) | curses.A_BOLD)

    help_line = (
        " ↑↓  d=details  u/s=up  t/x=down  r=restart  f=rescan  /=filter  ?=help  q=quit "
    )
    stdscr.addstr(1, 0, help_line[: w - 1], curses.A_DIM)

    list_w = max(28, min(42, w // 3))
    log_x = list_w + 1
    log_w = w - log_x - 1
    body_top = 2
    body_h = h - 4
    if body_h < 3:
        body_h = 3

    for y in range(body_top, body_top + body_h):
        if log_x < w:
            try:
                stdscr.addch(y, list_w, curses.ACS_VLINE)
            except curses.error:
                pass

    n = len(state.filtered)
    if n == 0:
        stdscr.addstr(body_top, 1, "(no services — press f)")
    else:
        view = body_h
        start = 0
        if state.selected >= view:
            start = state.selected - view + 1
        for i in range(view):
            idx = start + i
            y = body_top + i
            if idx >= n:
                break
            svc = state.filtered[idx]
            mark = {"run": "●", "down": "○", "fail": "!", "finish": "◆"}.get(
                svc.state, "?"
            )
            pid = f" {svc.pid}" if svc.pid else ""
            sec = f" {svc.seconds}s" if svc.seconds else ""
            den = " 🔒" if svc.access == "denied" and svc.state == "fail" else ""
            label = f"{mark} {svc.name}{pid}{sec}{den}"
            label = label[: list_w - 2].ljust(list_w - 2)
            attr = curses.color_pair(STATE_ATTR.get(svc.state, 5))
            if idx == state.selected and state.focus == "list":
                attr = curses.color_pair(6) | curses.A_BOLD
            elif svc.name in state.new_flash:
                attr = curses.color_pair(7) | curses.A_BOLD
            try:
                stdscr.addstr(y, 1, label, attr)
            except curses.error:
                pass

    svc = selected_service(state)
    log_title = f" log: {svc.name if svc else '—'} "
    if svc and svc.log_path:
        log_title += f"{svc.log_path} "
    try:
        stdscr.addstr(
            body_top,
            log_x + 1,
            log_title[: log_w - 1],
            curses.A_BOLD if state.focus == "log" else curses.A_DIM,
        )
    except curses.error:
        pass

    log_body_h = body_h - 1
    lines = state.log_lines
    if state.log_scroll == 0:
        view_lines = lines[-log_body_h:] if len(lines) > log_body_h else lines
    else:
        end = max(0, len(lines) - state.log_scroll)
        start_l = max(0, end - log_body_h)
        view_lines = lines[start_l:end]
    for i, line in enumerate(view_lines[:log_body_h]):
        try:
            stdscr.addstr(
                body_top + 1 + i,
                log_x + 1,
                line.replace("\t", " ")[: log_w - 2],
            )
        except curses.error:
            pass

    msg = ""
    if now < state.message_until:
        msg = state.message
    elif state.focus == "filter":
        msg = f"Filter: {state.filter_buf}_  (Enter apply, Esc cancel)"
    elif not is_root():
        msg = (svc.status_raw if svc else "ready") + "  |  tip: sudo runit-tui for full control"
    else:
        msg = (svc.status_raw if svc else "ready") or "ready"
    try:
        stdscr.addstr(h - 2, 0, f" {msg} "[: w - 1].ljust(w - 1), curses.color_pair(9))
        stdscr.addstr(
            h - 1,
            0,
            f" r=restart  d=details  t=down  f=rescan NEW  scan {int(now - state.last_scan_at)}s ago "[
                : w - 1
            ].ljust(w - 1),
            curses.A_DIM,
        )
    except curses.error:
        pass

    if state.focus == "detail":
        draw_overlay(
            stdscr,
            state.detail_lines,
            state.detail_scroll,
            f"DETAILS · {svc.name if svc else ''}",
        )
    elif state.focus == "help":
        draw_overlay(stdscr, HELP_TEXT, 0, "HELP")

    stdscr.refresh()


def main_loop(stdscr: "curses._CursesWindow", svdir: Path) -> None:
    curses.curs_set(0)
    stdscr.nodelay(True)
    stdscr.timeout(200)
    init_colors()

    state = AppState(svdir=svdir, elevated=is_root())
    rescan(state, force_status=True)
    refresh_log(state)
    if not is_root():
        state.message = (
            "Running as user — status via supervise files; "
            "use sudo runit-tui for up/down/restart without denial"
        )
        state.message_until = time.time() + 6.0

    while True:
        now = time.time()

        if state.focus not in ("filter", "detail", "help"):
            if now - state.last_scan_at >= RESCAN_INTERVAL_S:
                prev = (
                    state.filtered[state.selected].name
                    if state.filtered and state.selected < len(state.filtered)
                    else None
                )
                rescan(state, force_status=False)
                if prev:
                    for i, s in enumerate(state.filtered):
                        if s.name == prev:
                            state.selected = i
                            break
                refresh_log(state)

            if now - state.last_status_at >= STATUS_REFRESH_S:
                refresh_status(state.services, state.svdir)
                state.last_status_at = now

            if now - state.last_log_at >= LOG_REFRESH_S:
                refresh_log(state)
                state.last_log_at = now

        draw(stdscr, state)

        try:
            ch = stdscr.getch()
        except KeyboardInterrupt:
            break
        if ch == -1:
            continue

        # ── detail overlay ───────────────────────────────────────────────
        if state.focus == "detail":
            if ch in (27, ord("d"), ord("D"), ord("q")):
                if ch == ord("q"):
                    # q closes detail first; second q quits — only close here
                    pass
                state.focus = "list"
            elif ch in (curses.KEY_UP, ord("k")):
                state.detail_scroll = max(0, state.detail_scroll - 1)
            elif ch in (curses.KEY_DOWN, ord("j")):
                state.detail_scroll += 1
            elif ch == curses.KEY_PPAGE:
                state.detail_scroll = max(0, state.detail_scroll - 15)
            elif ch == curses.KEY_NPAGE:
                state.detail_scroll += 15
            elif ch == ord("g"):
                state.detail_scroll = 0
            continue

        if state.focus == "help":
            if ch in (27, ord("q"), ord("?"), ord("h"), ord("H")):
                state.focus = "list"
            continue

        if state.focus == "filter":
            if ch == 27:
                state.focus = "list"
                state.filter_buf = state.filter
            elif ch in (10, 13, curses.KEY_ENTER):
                state.filter = state.filter_buf
                apply_filter(state)
                state.focus = "list"
            elif ch in (curses.KEY_BACKSPACE, 127, 8):
                state.filter_buf = state.filter_buf[:-1]
            elif 32 <= ch < 127:
                state.filter_buf += chr(ch)
            continue

        # Global
        if ch in (ord("q"), ord("Q")):
            break
        if ch in (ord("?"), ord("h"), ord("H")):
            state.focus = "help"
            continue
        if ch in (ord("d"), ord("D")):
            open_details(state)
            continue
        if ch in (ord("f"), ord("F")):
            rescan(state, force_status=True)
            refresh_log(state)
            state.message = f"rescanned → {len(state.services)} services"
            state.message_until = time.time() + 3.0
        if ch == ord("/"):
            state.focus = "filter"
            state.filter_buf = state.filter
            continue
        if ch == 9:
            state.focus = "log" if state.focus == "list" else "list"
            continue

        if state.focus == "log":
            if ch in (curses.KEY_UP, ord("k")):
                state.log_scroll = min(len(state.log_lines), state.log_scroll + 1)
            elif ch in (curses.KEY_DOWN, ord("j")):
                state.log_scroll = max(0, state.log_scroll - 1)
            elif ch == curses.KEY_PPAGE:
                state.log_scroll = min(len(state.log_lines), state.log_scroll + 20)
            elif ch == curses.KEY_NPAGE:
                state.log_scroll = max(0, state.log_scroll - 20)
            elif ch == ord("G") or ch == ord(" "):
                state.log_scroll = 0
            elif ch in (10, 13, curses.KEY_ENTER, ord("l"), 27):
                state.focus = "list"
            continue

        n = len(state.filtered)
        if ch in (curses.KEY_UP, ord("k")):
            state.selected = max(0, state.selected - 1)
            state.log_scroll = 0
            refresh_log(state)
        elif ch in (curses.KEY_DOWN, ord("j")):
            state.selected = min(max(0, n - 1), state.selected + 1)
            state.log_scroll = 0
            refresh_log(state)
        elif ch == ord("g"):
            state.selected = 0
            refresh_log(state)
        elif ch == ord("G"):
            state.selected = max(0, n - 1)
            refresh_log(state)
        elif ch in (10, 13, curses.KEY_ENTER, ord("l")):
            state.focus = "log"
        elif ch in (ord("r"), ord("R")):
            sv_action(state, "restart")
        elif ch in (ord("u"), ord("s")):
            sv_action(state, "up")
        elif ch in (ord("t"), ord("x"), ord("X")):
            sv_action(state, "down")
        elif ch == ord("o"):
            sv_action(state, "once")
        elif ch == ord("c"):
            sv_action(state, "check")


def main() -> None:
    ap = argparse.ArgumentParser(
        description="runit TUI — auto-detects new services; d=details; r=restart"
    )
    ap.add_argument("--svdir", default=None, help="Service dir (default: auto)")
    ap.add_argument(
        "--rescan",
        type=float,
        default=RESCAN_INTERVAL_S,
        help=f"SVDIR rescan interval seconds (default {RESCAN_INTERVAL_S})",
    )
    args = ap.parse_args()
    set_rescan_interval(args.rescan)
    svdir = resolve_svdir(args.svdir)
    os.environ["SVDIR"] = str(svdir)
    curses.wrapper(lambda stdscr: main_loop(stdscr, svdir))


if __name__ == "__main__":
    main()
