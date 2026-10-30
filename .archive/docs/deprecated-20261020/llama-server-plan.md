# Llama server plan

Dedicated inference box: **Samsung (Tiger Lake, Iris Xe, 16 GB shared RAM)**.
Working machine: **this HP EliteBook 840 G6 (i7-8665U, UHD 620, 32 GB shared RAM)**.

Both iGPUs use **shared system RAM** (no dedicated VRAM).

## Roles

| Machine | Job |
|---|---|
| Samsung Tiger Lake + Iris Xe | Always-on 7–8B Q4 server, CLI only |
| HP 32 GB | Daily work; 13B Q4 locally; SSH/curl client to the Samsung |

Iris Xe (80 or 96 EU, Gen12, DP4A, ~1.7–2.0 TFLOPS) is worth using.
UHD 620 on the HP is not; keep the HP on CPU GGUF.

## Samsung OS

Headless **Debian 13 (or 12) netinst**. SSH only, no desktop.

Idle RAM is ~150–300 MB. That matters on 16 GB.

DietPi is fine if you want a TUI installer (still Debian).
Ubuntu Server 24.04 is fine if you want the Ollama curl installer.
Skip Alpine (Ollama wants glibc/systemd). Skip GNOME/KDE. Skip Docker-as-the-OS.

## What to run

**llama.cpp `llama-server` with Vulkan.** One GGUF, loaded at boot. No web UI.

Packages:

```bash
sudo apt update
sudo apt install -y mesa-vulkan-drivers vulkan-tools firmware-misc-nonfree \
  build-essential cmake git libvulkan-dev glslc
vulkaninfo --summary   # Intel Iris Xe
```

Build llama.cpp with `-DGGML_VULKAN=ON` (Tiger Lake also has AVX-512 + VNNI; leave those on).

Model: **Qwen2.5-7B or Llama 3.1 8B, Q4_K_M** in `/models`.
13B does not fit well on 16 GB. Keep 13B+ on the HP.

### A. Chat on the Samsung (SSH)

```bash
ssh samsung
llama-cli -m /models/qwen2.5-7b-instruct-q4_k_m.gguf \
  -ngl 99 --vulkan -c 4096 -cnv
```

### B. Daemon on Samsung, CLI on the HP (preferred)

Samsung systemd unit, starts at boot:

```bash
llama-server -m /models/qwen2.5-7b-instruct-q4_k_m.gguf \
  -ngl 99 --host 0.0.0.0 --port 8080 -c 4096
```

HP:

```bash
curl -s http://samsung:8080/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{"messages":[{"role":"user","content":"hello"}],"max_tokens":200}'
```

Other CLI clients pointed at `http://samsung:8080/v1`:

| Tool | Use |
|---|---|
| `llama-cli` | Interactive chat, no server |
| `llama-server` + `curl` | OpenAI-compatible HTTP |
| `llm` | `llm -m openai -o api_base http://samsung:8080/v1 'prompt'` |
| `mods` | Pipe stdin: `git diff \| mods -f 'review this'` |

Ollama (`ollama run`, `OLLAMA_VULKAN=1`, `OLLAMA_HOST=0.0.0.0`) is optional convenience. Extra RAM. Skip it for a dedicated 16 GB box.

## Laptop-as-server

- Stay plugged in. 80% charge limit if `samsung-galaxybook` / TLP exposes it.
- Lid closed: `HandleLidSwitch=ignore` in `/etc/systemd/logind.conf`.
- CPU governor `balanced` or `performance` on AC.
- Disable Bluetooth; use Ethernet if you can.
- Bind the API to LAN or Tailscale only. Do not expose port 8080 to the internet.

## Expected speed (7B Q4, AC power)

Samsung (Xe / AVX-512): roughly 12–25 tok/s.
HP (AVX2 CPU): roughly 8–15 tok/s for the same 7B; 13B Q4 is the reason to use the HP.

Same GGUF files work on both. Put 7–8B on the Samsung; keep 13B+ here.
