# Phase 0 — Benchmark

Machine: AMD Ryzen 7 3700X (8 cores / 16 threads), NVIDIA RTX 2060 SUPER 8 GB,
Windows 11. whisper.cpp via `whisper-rs 0.16.0`, Vulkan backend, MSVC build.
Tool: `cargo run --release -p bench -- <fixtures> <vad> <models...> [--cpu]`.

Fixtures (`tests/fixtures`) are generated with Windows SAPI voices (Hortense fr-FR,
Zira en-US): clean synthetic speech. **Real voices will give higher WER**; the
numbers rank models and measure latency, they do not predict real accuracy.

| Fixture | Duration |
|---|---|
| fr_short | 4.1 s |
| fr_long | 17.2 s |
| en | 9.5 s |
| silence, noise | 3 s |

## Pipeline decisions (measured)

| Decision | Measurement |
|---|---|
| Our own Silero VAD pass before Whisper; no speech → model not called | Without it: "Thank you.", "All right.", garbage tokens on silence/noise with every model. With it: empty output, 10 ms. |
| VAD on 1 CPU thread | 17 s audio: 68 ms (1 thread) vs 511 ms (8 threads). GPU VAD aborts inside whisper.cpp. |
| Whisper on GPU with 4 CPU threads | 1 thread 0.95 s, 2–4 threads 0.76–0.79 s, 8 threads 0.79 s (fr_long). |
| Greedy decoding on CPU, beam 5 on GPU | CPU small fr_long: 2.98 s greedy vs 4.23 s beam 5, same WER. |
| `audio_ctx` shrunk to audio length for base/small only | CPU small fr_short: 4.48 s → 0.53 s, same WER. **large-v3-turbo breaks** (fr_long WER 0 % → 37.5 %), so never for turbo. |
| MSVC kept (not clang-cl) | clang-cl only ~8 % faster on CPU; not worth a second toolchain. |

## Results — GPU (Vulkan), beam 5, full context

| Model | Load | fr_short | fr_long | en | WER fr / en |
|---|---|---|---|---|---|
| base-q5_1 | 0.4 s | 0.29 s | 1.22 s | 0.61 s | 12.5 % / 0 % |
| small-q5_1 | 0.3 s | 0.19 s | 0.76 s | 0.32 s | 0 % / 0 % |
| **large-v3-turbo-q5_0** | 0.7 s | **0.29 s** | **0.72 s** | **0.37 s** | 0 % / 0 % |
| large-v3-turbo-q8_0 | 1.0 s | 0.39 s | 1.23 s | 0.62 s | 0 % / 0 % |
| french-distil-dec16-q5_0 | 0.9 s | 0.49 s | 1.61 s | 1.19 s | 0 % / **72.7 %** |

## Results — CPU, 8 threads, greedy, `audio_ctx` where allowed

| Model | fr_short | fr_long | en | WER fr / en |
|---|---|---|---|---|
| **base-q5_1** | **0.18 s** | 1.05 s | 0.41 s | 12.5 % / 0 % |
| **small-q5_1** | **0.53 s** | 2.98 s | 1.14 s | 0 % / 0 % |
| large-v3-turbo-q5_0 (full ctx) | 19.8 s | 20.5 s | 19.8 s | 0 % / 0 % |

## Defaults

| Hardware | Model | Latency, 4 s phrase |
|---|---|---|
| GPU (Vulkan/Metal) | large-v3-turbo-q5_0 | 0.3 s (target < 0.7 s ✔) |
| CPU ≥ 8 threads | small-q5_1 | 0.5 s (target < 1.5 s ✔) |
| Smaller CPU | base-q5_1 | 0.2 s |

`large-v3-turbo-q8_0` brings nothing over Q5 on these fixtures and is slower: kept
as an option. The French specialist is only offered for French (it transcribes
English phonetically in French).
