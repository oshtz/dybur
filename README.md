# <img src="app-icon.png" alt="dybur icon" width="32" height="32" style="vertical-align: middle;"> dybur

Fast, local, private voice dictation for macOS and Windows.

> Talk into any text field. Instantly. Without cloud, accounts, or privacy trade-offs.
> [dybur.com](https://dybur.com)

## Features

- **100% Local** - Speech recognition runs entirely on your device using state-of-the-art ONNX models
- **Universal** - Inject text into any application via hotkey
- **Private** - No cloud, no accounts, no telemetry
- **Fast** - Sub-second transcription latency
- **Multilingual** - Supports 25 European languages with automatic detection
- **Smart** - Automatic punctuation and capitalization
- **VAD** - Voice Activity Detection filters silence for better accuracy

## Installation

Download the latest release from [GitHub Releases](https://github.com/oshtz/dybur/releases).

## Updates

dybur checks for updates automatically when the tray app starts. You can also run
a manual check from the tray menu with **Check for Updates...**.

Updates are installed from the public `dybur-update.json` manifest attached to the
latest GitHub release. The manifest points to the stable platform artifacts:

- `dybur-windows-x64.exe` for the portable Windows app
- `dybur-macos-arm64.dmg` for the macOS installer

Downloaded artifacts are verified with SHA-256 before installation. On Windows,
dybur exits and a helper process replaces the portable EXE, then relaunches the
app. On macOS, the helper mounts the DMG, replaces the installed `.app` bundle,
clears the quarantine attribute on the replacement bundle, detaches the DMG, and
relaunches dybur.

Set `DYBUR_DISABLE_AUTO_UPDATE=1` before launching dybur to skip automatic
startup checks. Manual checks from the tray menu still run.

For local release testing, set `DYBUR_UPDATE_MANIFEST_URL` to point dybur at a
test manifest instead of the public GitHub `latest` manifest. This override
applies to manual tray checks and to automatic startup checks in release builds.
Debug builds still skip automatic startup checks.

## Usage

1. Launch the app (or run `dybur start` from CLI)
2. Focus any text field
3. Press `Ctrl+Shift+Space` (default hotkey)
4. Speak
5. Press the hotkey again to stop (or release if using push-to-talk mode)
6. Text appears in the active field

### Recording Modes

dybur supports two recording modes:

- **Toggle** (default): Press the hotkey to start recording, press again to stop
- **Push-to-Talk**: Hold the hotkey to record, release to stop and transcribe

You can switch modes from the tray menu (Recording Mode) or by editing the config file.

### Voice Activity Detection (VAD)

VAD automatically filters silence and background noise before transcription, improving accuracy and reducing processing time. It uses the lightweight [Silero VAD](https://github.com/snakers4/silero-vad) model (~2MB) running locally via ONNX.

VAD is enabled by default. Toggle it from the tray menu or via CLI:

```sh
dybur vad          # Toggle VAD on/off
dybur vad on       # Enable VAD
dybur vad off      # Disable VAD
dybur vad status   # Show VAD settings
dybur vad threshold 0.6   # Set speech sensitivity (0.0-1.0)
dybur vad min-speech 250  # Set minimum speech duration in ms
dybur vad silence 1000    # Set silence split timeout in ms
```

All settings and controls are available from the tray menu or via CLI:

```sh
dybur start       # Start background service
dybur stop        # Stop service
dybur status      # Check service health (alias: s)
dybur settings    # Open config file (alias: config)
dybur doctor      # Run diagnostics (alias: diag)
dybur models      # Manage speech models (alias: m)
dybur devices     # Manage input devices (alias: d)
dybur vad         # Toggle Voice Activity Detection
```

## Configuration

Config file location:

- **macOS:** `~/Library/Application Support/dybur/config.json`
- **Windows:** `%APPDATA%\dybur\config.json`

```json
{
  "hotkey": "Ctrl+Shift+Space",
  "autoPunctuation": true,
  "sentenceCase": true,
  "silenceTimeoutMs": 1000,
  "model": "parakeet-tdt-v3-int8",
  "clipboardCleanup": true,
  "inputDevice": null,
  "recordingMode": "toggle",
  "vadEnabled": true,
  "vadThreshold": 0.5,
  "vadMinSpeechMs": 250,
  "gpuMode": "auto",
  "streamingEnabled": true
}
```

| Option             | Values                      | Description                                                     |
| ------------------ | --------------------------- | --------------------------------------------------------------- |
| `hotkey`           | Key combo                   | Global hotkey to trigger recording                              |
| `autoPunctuation`  | `true`/`false`              | Automatically add punctuation                                   |
| `sentenceCase`     | `true`/`false`              | Capitalize first letter of sentences                            |
| `silenceTimeoutMs` | Number                      | Minimum silence duration used to split VAD speech segments (ms) |
| `model`            | Model name                  | Speech recognition model to use                                 |
| `clipboardCleanup` | `true`/`false`              | Restore clipboard after text injection                          |
| `inputDevice`      | Device name or `null`       | Microphone to use; `null` uses the system default               |
| `recordingMode`    | `"toggle"`/`"push_to_talk"` | Recording behavior mode                                         |
| `vadEnabled`       | `true`/`false`              | Enable Voice Activity Detection                                 |
| `vadThreshold`     | `0.0`-`1.0`                 | VAD sensitivity (higher = stricter)                             |
| `vadMinSpeechMs`   | Number                      | Minimum speech duration to keep (ms)                            |
| `gpuMode`          | `"auto"`/`"cpu"`            | Use GPU acceleration when available or force CPU                |
| `streamingEnabled` | `true`/`false`              | Enable live preview for compatible streaming models             |

## Models

dybur supports multiple speech recognition models. You can switch models from the tray menu or via CLI:

```sh
dybur models list      # List available models
dybur models set       # Select a model interactively
dybur models candidates # Show experimental model candidates
```

| Model                         | Size    | Languages | Description                                             |
| ----------------------------- | ------- | --------- | ------------------------------------------------------- |
| `parakeet-tdt-v3-int8`        | ~670 MB | 25        | **Default.** Multilingual transducer, balanced accuracy |
| `nemotron-streaming-int8`     | ~660 MB | English   | Low-latency streaming transducer                        |
| `whisper-large-v3-turbo-int8` | ~1.1 GB | 99        | OpenAI Whisper, broad language support                  |
| `whisper-large-v3-turbo-fp16` | ~1.6 GB | 99        | Whisper FP16, higher accuracy                           |

Models are downloaded automatically on first use.

### Manual Model Provisioning

For offline or locked-down machines, pre-provision models from a connected machine:

1. On the connected machine, run `dybur models download <model-id>`.
2. Copy the downloaded model directory into the target machine's models directory:
   - macOS: `~/Library/Application Support/dybur/models/<model-id>`
   - Windows: `%APPDATA%\dybur\models\<model-id>`
3. If VAD is enabled, also copy `silero-vad` from the same `models` directory.
4. On the target machine, run `dybur models set <model-id>` and `dybur doctor`.

Keep the copied `metadata.json` file with each model directory; dybur uses it for status and cleanup.

### ASR Evaluation

Use the local scoring harness to compare saved model hypotheses across a repeatable sample set:

```sh
pnpm eval:asr:manifest benchmarks/asr/example.json --require-duration --require-tags
pnpm eval:asr:manifest benchmarks/asr/<run>.json --config benchmarks/asr/corpus-policy.example.json
pnpm eval:asr benchmarks/asr/example.json
pnpm eval:asr benchmarks/asr/example.json --format json --output benchmarks/asr/report.json --strict
pnpm eval:asr:gate benchmarks/asr/candidate-report.json --config benchmarks/asr/gates/candidate-promotion.example.json
```

The harness reports WER, CER, median latency, realtime factor, and per-tag summaries. See `docs/asr-evaluation.md` for the manifest shape, reusable corpus policy, and recommended sample set.

Experimental model candidates such as CoreML Parakeet, MLX Parakeet, Qwen3-ASR, and Moonshine are tracked separately from production model IDs. Use `dybur models candidates` to inspect them, `scripts/asr-candidates/` for benchmark wrappers, and `docs/model-candidate-evaluation.md` for the benchmark workflow.

Release smoke checks live in [docs/release-smoke-checklist.md](docs/release-smoke-checklist.md).

## Requirements

- **macOS:** 10.15+ (Catalina or later)
- **Windows:** 10/11
- **Microphone:** Required for dictation
- **Disk:** ~700 MB - 1.6 GB depending on speech model

### macOS Permissions

On first launch, macOS will prompt for the following:

- **Administrator Password** - To install the `dybur` CLI command to `/usr/local/bin` (one-time setup, can be skipped)
- **Microphone Access** - Required for voice recording during dictation
- **Accessibility** - Required for injecting text into applications (System Settings → Privacy & Security → Accessibility)

## Development

### Prerequisites

- Node.js >= 18.0.0
- pnpm 8.10.0+
- Rust (for building the tray application)

### Project Structure

```
dybur/
├── apps/
│   ├── cli/               # Rust CLI binary (sidecar for tray app)
│   └── tray/              # Tauri 2.0 tray application
├── packages/
│   ├── cli/               # Node.js CLI (@dybur/cli)
│   ├── config/            # Configuration management
│   └── core/              # Core business logic
└── scripts/               # Build and utility scripts
```

### Setup

```sh
# Install dependencies
pnpm install

# Build all packages
pnpm build

# Run tests
pnpm test

# Lint code
pnpm lint

# Type check
pnpm typecheck
```

### Building the Tray App

```sh
cd apps/tray
pnpm tauri build
```

## Privacy

- All speech processing happens locally on your device
- Audio never leaves your computer
- No cloud services, no accounts required
- No telemetry or analytics
- Logs contain no speech content

## License

MIT
