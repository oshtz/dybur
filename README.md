# <img src="app-icon.png" alt="dybur icon" width="32" height="32" style="vertical-align: middle;"> dybur

Fast, local, private voice dictation for macOS and Windows.

> Talk into any text field. Instantly. Without cloud, accounts, or privacy trade-offs.
[dybur.com](https://dybur.com)

## Features

- **100% Local** - Speech recognition runs entirely on your device using [NVIDIA Parakeet](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3)
- **Universal** - Inject text into any application via hotkey
- **Private** - No cloud, no accounts, no telemetry
- **Fast** - Sub-second transcription latency
- **Multilingual** - Supports 25 European languages with automatic detection
- **Smart** - Automatic punctuation and capitalization
- **VAD** - Voice Activity Detection filters silence for better accuracy

## Installation

Download the latest release from [GitHub Releases](https://github.com/oshtz/dybur/releases).

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
  "model": "parakeet-tdt-0.6b-v3-onnx",
  "clipboardCleanup": true,
  "recordingMode": "toggle",
  "vadEnabled": true,
  "vadThreshold": 0.5,
  "vadMinSpeechMs": 250
}
```

| Option | Values | Description |
|--------|--------|-------------|
| `hotkey` | Key combo | Global hotkey to trigger recording |
| `autoPunctuation` | `true`/`false` | Automatically add punctuation |
| `sentenceCase` | `true`/`false` | Capitalize first letter of sentences |
| `silenceTimeoutMs` | Number | Silence detection timeout (ms) |
| `model` | Model name | Speech recognition model to use |
| `clipboardCleanup` | `true`/`false` | Restore clipboard after text injection |
| `recordingMode` | `"toggle"`/`"push_to_talk"` | Recording behavior mode |
| `vadEnabled` | `true`/`false` | Enable Voice Activity Detection |
| `vadThreshold` | `0.0`-`1.0` | VAD sensitivity (higher = stricter) |
| `vadMinSpeechMs` | Number | Minimum speech duration to keep (ms) |

## Requirements

- **macOS:** 10.15+ (Catalina or later)
- **Windows:** 10/11
- **Microphone:** Required for dictation
- **Disk:** ~700 MB for speech model (INT8 quantized)

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
