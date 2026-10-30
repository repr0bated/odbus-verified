# Transcript: GPU / local inference / Samsung llama server

Saved from the live-USB session on the HP EliteBook 840 G6 (Omarchy NVMe).
Companion plan: `~/llama-server-plan.md`

---

**User:** do analysis on gpu and local inference on this machine

**Assistant:** Inspected the box. CachyOS live USB. Intel i7-8665U, Intel UHD Graphics 620, 32 GB RAM, no discrete NVIDIA/AMD GPU on PCI (`nvidia-smi` was leftover ISO packages). Live overlay root, internal 1 TB NVMe LUKS unmounted. No Ollama/llama.cpp/vLLM installed.

---

**User:** it is not configured, i just want capabilities for what can be installed and run. is it shared ram?

**Assistant:** Yes. The GPU is **shared RAM** — no discrete GPU with its own VRAM.

Hardware: HP EliteBook 840 G6, i7-8665U (4C/8T, 1.9–4.8 GHz), UHD 620 (24 EUs, 300–1150 MHz), 32 GB unified with the iGPU, ~15 W U-series, 1 TB NVMe.

`glxinfo`: Unified memory yes, ~31.9 GB advertised as video memory (that is the system RAM pool). Model weights, KV cache, OS, and browser all compete for the same 32 GB. Practical working set ~24–28 GB.

UHD 620 is a display/media iGPU, not an AI accelerator. OpenGL already works. Vulkan after `vulkan-intel`. OpenCL/Level Zero after `intel-compute-runtime`. No tensor cores, XMX, AVX-512 VNNI, or AMX. For LLMs, treat the **CPU as the primary engine**.

Best fit: llama.cpp / Ollama / KoboldCpp / LocalAI (GGUF).

| Model | Quant | 32 GB shared? | CPU Q4 speed |
|---|---|---|---|
| 1–3B | Q4/Q5/Q8 | easily | ~25–60 tok/s |
| 7–9B | Q4_K_M / Q5 | comfortable | ~8–15 tok/s |
| 13–14B | Q4 | yes | ~4–8 tok/s |
| 32B | Q3/Q4 | tight | ~1.5–4 tok/s |
| 70B | Q2/IQ2 | barely | <1 tok/s |

iGPU offload on UHD 620 rarely beats AVX2 CPU decode. Whisper/embeddings: yes, CPU. SD: possible but slow. vLLM/TensorRT/CUDA: no NVIDIA GPU. PyTorch CPU possible; this live Python is 3.14.

CachyOS packages: ollama or llama.cpp AVX2; optional `vulkan-intel`, `intel-compute-runtime`, `intel-media-driver`, `level-zero`. Skip NVIDIA/CUDA/ROCm. Use AC power, not `powersave` on battery.

Bottom line: shared 32 GB, Intel iGPU only. 7–9B Q4 well, 13B acceptable, 32B a stretch.

---

**User:** what do you mean igpu is weak?

**Assistant:** Weak **for local inference**. Fine at driving the screen and decoding video. Poor LLM accelerator.

UHD 620: 24 EUs, 192 shaders, Gen9.5, ~0.44 TFLOPS FP32 / ~0.88 TFLOPS FP16. No tensor cores, XMX, INT8 DP4A. Quantized Q4 matmul is ordinary FMAs.

No private memory bus. Weights stream from the same DDR4 as the CPU (~38 GB/s dual-channel DDR4-2400). An RTX 4060 laptop is ~190–250 GB/s. Token generation is “read the whole model per token,” so speed tracks bandwidth. The iGPU does not get a bandwidth bump over the CPU.

Token generation: memory bandwidth, same ~38 GB/s as CPU.
Prompt processing: dense matmul, 0.44 TFLOPS, no matrix units.
Q4/Q5 kernels: emulated on Gen9, thin Vulkan/SYCL.

llama.cpp AVX2 CPU kernels for this Intel gen are mature. Vulkan/SYCL on Gen9 is the opposite, and iGPU+CPU share the 15 W package.

Scale: this iGPU ~0.44 TFLOPS; modern Arc iGPU several TFLOPS + XMX; RTX 4060 laptop ~15–20 TFLOPS FP32 plus tensor cores.

It can run compute. It cannot make 7B–13B feel GPU-fast. Useful engine: CPU + 32 GB shared RAM.

---

**User:** compare to my samsung with iris gpu 16GB

**Assistant:** Both use **shared system RAM**. The Samsung’s 16 GB is the same kind of pool, not 16 GB of VRAM.

Samsung notebooks listed as Iris + 16 GB are almost always **Iris Xe** (Galaxy Book 11th–13th gen).

| | HP EliteBook 840 G6 | Typical Samsung + Iris Xe, 16 GB |
|---|---|---|
| Memory | 32 GB DDR4-2400, ~38 GB/s | 16 GB LPDDR4X/LPDDR5, often ~60–70+ GB/s |
| GPU | UHD 620, 24 EU, Gen9.5, ~0.44 TFLOPS | Iris Xe, 80–96 EU, Gen12, ~1.6–2.2 TFLOPS |
| GPU extras | FMAs only | DP4A INT8 |
| CPU | i7-8665U, AVX2, ~15 W | Tiger/Alder/Raptor U, AVX-512 and/or VNNI, often 28 W burst |
| Model headroom | ~24–28 GB | ~8–11 GB after Windows |

Samsung iGPU is the faster graphics chip. HP has the bigger tank.

Iris Xe is 4–5× the EUs and peak math. llama.cpp Vulkan on Xe is a real path. On UHD 620 it usually is not.

Samsung 16 GB: 3B easy; **7–8B Q4 daily driver**; 13B Q4 only if everything else is closed; 14B/32B no.

HP 32 GB: 7–8B comfortable but slower tok/s; **13B Q4 is this machine’s job**; 32B Q3/Q4 possible.

Rough 7B Q4 on AC: Samsung ~12–25 tok/s; HP ~8–15 tok/s.

Use Samsung for 7–8B with Vulkan. Use HP for 13B CPU GGUF.

---

**User:** tiger lake

**Assistant:** Tiger Lake (11th-gen, 2020) vs this HP: same 4-core / 8-thread class, different engine.

Samsung wins GPU, SIMD, memory **bandwidth**. HP wins memory **capacity**.

If Task Manager says Iris Xe Graphics: G7 (80 EU i5-1135G7 or 96 EU i7-1165G7). G4 is UHD 48 EU.

Iris Xe ~3–4× EUs, ~4–5× peak math vs UHD 620. Gen12 DP4A. Enable Vulkan on Xe.

Token gen still follows bandwidth / model size. Tiger Lake LPDDR4X typically 1.5–1.8× this EliteBook’s DDR4-2400, so a 7B Q4 that fits both should decode faster on the Samsung.

Both 4C/8T. Tiger Lake adds AVX-512 and VNNI. llama.cpp CPU Q4/Q8 uses that. 7B Q4 CPU-only on Samsung often ~1.3–2× this HP.

16 GB shared is tight after OS + 4.5 GB 7B file. Long chats/RAG blow KV cache. HP can keep 13B resident.

Samsung: daily 7–8B Q4, Ollama or llama.cpp Vulkan.
HP: 13B Q4, CPU GGUF, leave the iGPU out.

---

**User:** not running windows. is there a lightweight os to use if i set the samsung as dedicated llama server?

**Assistant:** Yes. Headless **Debian** + **llama.cpp Vulkan server**. Lightest stack that still uses Iris Xe well.

**Debian 13 (or 12) netinst, no desktop.** SSH only. Idle ~150–300 MB.

DietPi if you want a TUI installer (still Debian).
Ubuntu Server 24.04 if you want `curl | sh` Ollama.
Skip Alpine (Ollama wants glibc/systemd). Skip a desktop. Skip Proxmox/Docker-as-OS on 16 GB.

CachyOS minimal works; a 24/7 laptop is happier on Debian’s slower cadence.

Run **llama.cpp `llama-server` with Vulkan**, one GGUF, always loaded. Ollama costs extra RAM; on 16 GB pin one 7–8B Q4.

```bash
sudo apt install -y mesa-vulkan-drivers vulkan-tools firmware-misc-nonfree \
  build-essential cmake git libvulkan-dev glslc
```

Build with `-DGGML_VULKAN=ON`. Serve Qwen2.5-7B or Llama 3.1 8B Q4_K_M. Bind `0.0.0.0:8080`. Suggested putting Open WebUI on the HP (32 GB), not on the Samsung.

If Ollama: `OLLAMA_VULKAN=1` and `OLLAMA_HOST=0.0.0.0`.

Laptop-as-server: plugged in, 80% charge limit if exposed, `HandleLidSwitch=ignore`, TLP, disable BT, Tailscale/LAN only.

7B Q4 ~4.5 GB plus cache fits. 13B does not.

---

**User:** u dont want web ui i want to use in cli

**Assistant:** Skip the web UI. Headless llama box from a terminal.

**A. SSH in and chat on the Samsung**

```bash
ssh samsung
llama-cli -m /models/qwen2.5-7b-instruct-q4_k_m.gguf \
  -ngl 99 --vulkan -c 4096 -cnv
```

**B. Server on Samsung, CLI on this HP**

Samsung:

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

CLI tools: `llama-cli`, `llama-server` + `curl`, `ollama run`, `llm`, `mods`.

Debian netinst, no desktop, SSH only, Vulkan llama.cpp, one 7–8B Q4 in `/models`. Either `ssh samsung` and `-cnv`, or point `llm`/`curl` at `http://samsung:8080/v1`. No browser, no Open WebUI.

---

**User:** that was a diversion os why i am booted into live usb. we fixed secure boot but it wont mount root luks. …

(Boot/LUKS recovery from here on is a separate thread, not this plan.)
