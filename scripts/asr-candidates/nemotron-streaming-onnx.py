#!/usr/bin/env python3
"""Benchmark wrapper for dybur's installed Nemotron streaming ONNX model."""

from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path
from typing import Any


DEFAULT_MODEL_ID = "nemotron-streaming-int8"
REQUIRED_FILES = (
    "encoder.int8.onnx",
    "decoder.int8.onnx",
    "joiner.int8.onnx",
    "tokens.txt",
)
SAMPLE_RATE = 16000
N_FFT = 512
HOP_LENGTH = 160
WIN_LENGTH = 400
N_MELS = 128
MEL_FMIN = 0.0
MEL_FMAX = 8000.0
WORD_BOUNDARY = "\u2581"


DEFAULT_STREAMING_METADATA = {
    "window_size": 112,
    "chunk_shift": 112,
    "cache_last_channel_dims": [1, 24, 70, 1024],
    "cache_last_time_dims": [1, 24, 1024, 70],
}


def fail(message: str, exit_code: int = 1) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(exit_code)


def import_runtime() -> tuple[Any, Any, Any, Any]:
    missing = []
    modules = {}

    for module_name in ("librosa", "numpy", "onnxruntime", "soundfile"):
        try:
            modules[module_name] = __import__(module_name)
        except ImportError:
            missing.append(module_name)

    if missing:
        fail(
            "Missing dependency: "
            + ", ".join(missing)
            + ". Install `onnxruntime`, `soundfile`, and `librosa` in the "
            "isolated benchmark Python environment.",
            2,
        )

    return (
        modules["librosa"],
        modules["numpy"],
        modules["onnxruntime"],
        modules["soundfile"],
    )


def default_model_dir(model_id: str) -> Path:
    return Path.home() / ".dybur" / "models" / model_id


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run dybur's installed Nemotron streaming ONNX model on one audio file."
    )
    parser.add_argument("audio", nargs="?", help="Path to a local audio file.")
    parser.add_argument(
        "--manifest",
        default=None,
        help="ASR manifest with samples[] to run in one warmed model process.",
    )
    parser.add_argument(
        "--output",
        default=None,
        help="JSON output path for --manifest batch mode.",
    )
    parser.add_argument(
        "--model-id",
        default=DEFAULT_MODEL_ID,
        help=f"dybur model id under ~/.dybur/models. Default: {DEFAULT_MODEL_ID}",
    )
    parser.add_argument(
        "--model-dir",
        default=None,
        help="Explicit local model directory. Overrides --model-id.",
    )
    parser.add_argument(
        "--provider",
        choices=("auto", "cpu", "cuda", "dml"),
        default="auto",
        help="ONNX Runtime execution provider.",
    )
    parser.add_argument(
        "--max-symbols-per-step",
        type=int,
        default=10,
        help="Maximum emitted tokens per acoustic frame.",
    )
    parser.add_argument(
        "--max-total-tokens",
        type=int,
        default=500,
        help="Maximum emitted tokens for one utterance.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help='Print {"text": "..."} instead of plain text.',
    )
    parser.add_argument(
        "--preflight",
        action="store_true",
        help="Check runtime imports, model files, and ONNX sessions without audio.",
    )
    args = parser.parse_args()
    if bool(args.manifest) != bool(args.output):
        parser.error("--manifest and --output must be used together")
    if args.manifest and args.audio:
        parser.error("audio positional input cannot be combined with --manifest")
    if not args.preflight and not args.audio and not args.manifest:
        parser.error("audio or --manifest is required unless --preflight is used")
    if args.max_symbols_per_step <= 0:
        parser.error("--max-symbols-per-step must be positive")
    if args.max_total_tokens <= 0:
        parser.error("--max-total-tokens must be positive")
    return args


def resolve_model_dir(args: argparse.Namespace) -> Path:
    model_dir = Path(args.model_dir).expanduser() if args.model_dir else default_model_dir(args.model_id)
    return model_dir.resolve()


def validate_model_dir(model_dir: Path) -> None:
    if not model_dir.exists():
        fail(f"Model directory not found: {model_dir}", 2)

    missing = [name for name in REQUIRED_FILES if not (model_dir / name).exists()]
    if missing:
        fail(f"Model directory is missing required file(s): {', '.join(missing)}", 2)


def select_providers(onnxruntime: Any, requested: str) -> list[str]:
    available = set(onnxruntime.get_available_providers())
    requested_map = {
        "cpu": "CPUExecutionProvider",
        "cuda": "CUDAExecutionProvider",
        "dml": "DmlExecutionProvider",
    }

    if requested != "auto":
        provider = requested_map[requested]
        if provider not in available:
            fail(f"Requested provider is not available: {provider}", 2)
        return [provider]

    preferred = ("DmlExecutionProvider", "CUDAExecutionProvider", "CPUExecutionProvider")
    providers = [provider for provider in preferred if provider in available]
    if not providers:
        fail("No compatible ONNX Runtime execution provider is available", 2)
    return providers


def parse_vocab_line(line: str) -> tuple[str, int | None] | None:
    text = line.strip()
    if not text:
        return None

    token_part, separator, id_part = text.rpartition(" ")
    if separator:
        try:
            return token_part, int(id_part)
        except ValueError:
            pass
    return text, None


def load_vocabulary(path: Path) -> list[str]:
    vocab: list[str] = []
    with path.open("r", encoding="utf-8") as handle:
        for line in handle:
            parsed = parse_vocab_line(line)
            if parsed is None:
                continue
            token, token_id = parsed
            if token_id is None:
                vocab.append(token)
                continue
            if len(vocab) <= token_id:
                vocab.extend([""] * (token_id + 1 - len(vocab)))
            vocab[token_id] = token

    if not vocab:
        fail(f"Vocabulary file is empty: {path}", 2)
    return vocab


def find_token_id(vocab: list[str], token: str) -> int | None:
    try:
        return vocab.index(token)
    except ValueError:
        return None


def decode_tokens(tokens: list[int], vocab: list[str]) -> str:
    text = ""
    for token_id in tokens:
        if token_id < 0 or token_id >= len(vocab):
            continue
        token = vocab[token_id]
        if not token:
            continue
        if token.startswith(WORD_BOUNDARY):
            if text:
                text += " "
            text += token[len(WORD_BOUNDARY) :]
        elif token == "<space>":
            text += " "
        elif not token.startswith("<"):
            text += token
    return text.strip()


def load_audio(audio_path: str, librosa: Any, numpy: Any, soundfile: Any) -> Any:
    audio, sample_rate = soundfile.read(audio_path, dtype="float32")
    if getattr(audio, "ndim", 1) > 1:
        audio = audio.mean(axis=1)
    if sample_rate != SAMPLE_RATE:
        audio = librosa.resample(audio, orig_sr=sample_rate, target_sr=SAMPLE_RATE)
    return audio.astype(numpy.float32)


def create_mel_filterbank(numpy: Any) -> Any:
    n_freqs = N_FFT // 2 + 1

    def hz_to_mel(hz: float) -> float:
        return 2595.0 * numpy.log10(1.0 + hz / 700.0)

    def mel_to_hz(mel: float) -> float:
        return 700.0 * (10.0 ** (mel / 2595.0) - 1.0)

    mel_min = hz_to_mel(MEL_FMIN)
    mel_max = hz_to_mel(MEL_FMAX)
    mel_points = [mel_to_hz(mel_min + (mel_max - mel_min) * i / (N_MELS + 1)) for i in range(N_MELS + 2)]
    fft_freqs = numpy.arange(n_freqs, dtype=numpy.float32) * SAMPLE_RATE / N_FFT
    filterbank = numpy.zeros((N_MELS, n_freqs), dtype=numpy.float32)

    for mel_idx in range(N_MELS):
        left = mel_points[mel_idx]
        center = mel_points[mel_idx + 1]
        right = mel_points[mel_idx + 2]
        rising = (fft_freqs >= left) & (fft_freqs <= center)
        falling = (fft_freqs > center) & (fft_freqs <= right)
        filterbank[mel_idx, rising] = (fft_freqs[rising] - left) / (center - left)
        filterbank[mel_idx, falling] = (right - fft_freqs[falling]) / (right - center)

    return filterbank


def deterministic_noise(numpy: Any, length: int) -> Any:
    seed = 12345
    values = numpy.empty(length, dtype=numpy.float32)
    for idx in range(length):
        seed = (seed * 1103515245 + 12345) & 0xFFFFFFFF
        values[idx] = ((seed >> 16) / 65536.0) * 2.0 - 1.0
    return values


def compute_mel_spectrogram(audio: Any, numpy: Any, mel_filterbank: Any) -> Any:
    if len(audio) == 0:
        audio = numpy.zeros(1, dtype=numpy.float32)

    preemphasized = audio.astype(numpy.float32).copy()
    if len(preemphasized) > 1:
        preemphasized[1:] = preemphasized[1:] - 0.97 * preemphasized[:-1]
    preemphasized += 1e-5 * deterministic_noise(numpy, len(preemphasized))

    padded = numpy.pad(preemphasized, (N_FFT // 2, N_FFT // 2), mode="constant")
    n_frames = 1 + max(0, (len(padded) - N_FFT) // HOP_LENGTH)
    window = numpy.zeros(N_FFT, dtype=numpy.float32)
    window[:WIN_LENGTH] = 0.5 * (
        1.0 - numpy.cos(2.0 * numpy.pi * numpy.arange(WIN_LENGTH, dtype=numpy.float32) / WIN_LENGTH)
    )

    power_spec = numpy.zeros((N_FFT // 2 + 1, n_frames), dtype=numpy.float32)
    for frame_idx in range(n_frames):
        start = frame_idx * HOP_LENGTH
        frame = padded[start : start + N_FFT]
        if len(frame) < N_FFT:
            frame = numpy.pad(frame, (0, N_FFT - len(frame)), mode="constant")
        spectrum = numpy.fft.rfft(frame * window, n=N_FFT)
        power_spec[:, frame_idx] = numpy.abs(spectrum).astype(numpy.float32) ** 2

    mel_spec = mel_filterbank @ power_spec
    return numpy.log10(numpy.maximum(mel_spec, 1e-10)).astype(numpy.float32)


def read_streaming_metadata(encoder: Any) -> dict[str, Any]:
    metadata = dict(DEFAULT_STREAMING_METADATA)
    try:
        custom = encoder.get_modelmeta().custom_metadata_map
    except Exception:
        custom = {}

    def int_value(name: str, default: int) -> int:
        try:
            return int(custom.get(name, default))
        except (TypeError, ValueError):
            return default

    metadata["window_size"] = int_value("window_size", metadata["window_size"])
    metadata["chunk_shift"] = int_value("chunk_shift", metadata["chunk_shift"])
    metadata["cache_last_channel_dims"] = [
        1,
        int_value("cache_last_channel_dim1", metadata["cache_last_channel_dims"][1]),
        int_value("cache_last_channel_dim2", metadata["cache_last_channel_dims"][2]),
        int_value("cache_last_channel_dim3", metadata["cache_last_channel_dims"][3]),
    ]
    metadata["cache_last_time_dims"] = [
        1,
        int_value("cache_last_time_dim1", metadata["cache_last_time_dims"][1]),
        int_value("cache_last_time_dim2", metadata["cache_last_time_dims"][2]),
        int_value("cache_last_time_dim3", metadata["cache_last_time_dims"][3]),
    ]
    return metadata


def load_sessions(model_dir: Path, onnxruntime: Any, providers: list[str]) -> tuple[Any, Any, Any]:
    session_options = onnxruntime.SessionOptions()
    encoder = onnxruntime.InferenceSession(
        str(model_dir / "encoder.int8.onnx"),
        sess_options=session_options,
        providers=providers,
    )
    decoder = onnxruntime.InferenceSession(
        str(model_dir / "decoder.int8.onnx"),
        sess_options=session_options,
        providers=providers,
    )
    joiner = onnxruntime.InferenceSession(
        str(model_dir / "joiner.int8.onnx"),
        sess_options=session_options,
        providers=providers,
    )
    return encoder, decoder, joiner


def outputs_by_name(session: Any, outputs: list[Any]) -> dict[str, Any]:
    return {meta.name: value for meta, value in zip(session.get_outputs(), outputs)}


def run_decoder_step(
    numpy: Any,
    decoder: Any,
    token: int,
    decoder_h_state: Any,
    decoder_c_state: Any,
) -> tuple[int, Any, Any, Any]:
    outputs = decoder.run(
        None,
        {
            "targets": numpy.array([[token]], dtype=numpy.int32),
            "target_length": numpy.array([1], dtype=numpy.int32),
            "states.1": decoder_h_state,
            "onnx::Slice_3": decoder_c_state,
        },
    )
    named = outputs_by_name(decoder, outputs)
    dec_output = named["outputs"].reshape(-1).astype(numpy.float32)
    decoder_dim = int(named["outputs"].shape[1])
    if "states" in named:
        decoder_h_state = named["states"]
    if "162" in named:
        decoder_c_state = named["162"]
    return decoder_dim, dec_output, decoder_h_state, decoder_c_state


def update_encoder_cache(numpy: Any, encoder: Any, outputs: list[Any]) -> tuple[Any, Any, int]:
    named = outputs_by_name(encoder, outputs)
    cache_channel = named["cache_last_channel_next"].astype(numpy.float32)
    cache_time = named["cache_last_time_next"].astype(numpy.float32)
    cache_len = int(named["cache_last_channel_next_len"].reshape(-1)[0])
    return cache_channel, cache_time, cache_len


def transcribe_loaded(
    args: argparse.Namespace,
    runtime: tuple[Any, Any, Any, Any],
    loaded_model: tuple[Path, Any, Any, Any, list[str], dict[str, Any], Any],
    audio_path: str,
) -> str:
    librosa, numpy, _, soundfile = runtime
    _, encoder, decoder, joiner, vocab, metadata, mel_filterbank = loaded_model
    audio = load_audio(audio_path, librosa, numpy, soundfile)
    mel_spec = compute_mel_spectrogram(audio, numpy, mel_filterbank)

    chunk_size = int(metadata["window_size"])
    chunk_shift = int(metadata["chunk_shift"])
    cache_channel = numpy.zeros(metadata["cache_last_channel_dims"], dtype=numpy.float32)
    cache_time = numpy.zeros(metadata["cache_last_time_dims"], dtype=numpy.float32)
    cache_len = 0

    decoder_h_state = numpy.zeros((2, 1, 640), dtype=numpy.float32)
    decoder_c_state = numpy.zeros((2, 1, 640), dtype=numpy.float32)
    blank_id = find_token_id(vocab, "<blk>")
    if blank_id is None:
        blank_id = find_token_id(vocab, "<blank>")
    if blank_id is None:
        blank_id = len(vocab) - 1

    decoder_dim, current_dec_output, decoder_h_state, decoder_c_state = run_decoder_step(
        numpy,
        decoder,
        blank_id,
        decoder_h_state,
        decoder_c_state,
    )

    tokens: list[int] = []
    offset = 0
    chunk_idx = 0
    total_frames = mel_spec.shape[1]

    while offset < total_frames and len(tokens) < args.max_total_tokens:
        chunk_end = min(offset + chunk_size, total_frames)
        chunk_len = chunk_end - offset
        if chunk_len < 16:
            break

        chunk = mel_spec[:, offset:chunk_end][None, :, :].astype(numpy.float32)
        encoder_outputs = encoder.run(
            None,
            {
                "audio_signal": chunk,
                "length": numpy.array([chunk_len], dtype=numpy.int64),
                "cache_last_channel": cache_channel,
                "cache_last_time": cache_time,
                "cache_last_channel_len": numpy.array([cache_len], dtype=numpy.int64),
            },
        )
        encoder_named = outputs_by_name(encoder, encoder_outputs)
        encoded = encoder_named["outputs"]
        time_frames = encoded.shape[2]
        encoder_dim = encoded.shape[1]

        for frame_idx in range(time_frames):
            if len(tokens) >= args.max_total_tokens:
                break

            enc_frame = encoded[:, :, frame_idx : frame_idx + 1].astype(numpy.float32)
            if enc_frame.shape != (1, encoder_dim, 1):
                enc_frame = enc_frame.reshape(1, encoder_dim, 1)

            symbols_emitted = 0
            while symbols_emitted < args.max_symbols_per_step and len(tokens) < args.max_total_tokens:
                dec_frame = current_dec_output.reshape(1, decoder_dim, 1).astype(numpy.float32)
                joiner_outputs = joiner.run(
                    None,
                    {
                        "encoder_outputs": enc_frame,
                        "decoder_outputs": dec_frame,
                    },
                )
                logits = joiner_outputs[0].reshape(-1)
                next_token = int(numpy.argmax(logits))
                if next_token == blank_id:
                    break

                tokens.append(next_token)
                symbols_emitted += 1
                decoder_dim, current_dec_output, decoder_h_state, decoder_c_state = run_decoder_step(
                    numpy,
                    decoder,
                    next_token,
                    decoder_h_state,
                    decoder_c_state,
                )

        cache_channel, cache_time, cache_len = update_encoder_cache(numpy, encoder, encoder_outputs)
        offset += chunk_shift
        chunk_idx += 1

    return decode_tokens(tokens, vocab)


def load_model(
    args: argparse.Namespace,
    runtime: tuple[Any, Any, Any, Any],
) -> tuple[Path, Any, Any, Any, list[str], dict[str, Any], Any]:
    _, numpy, onnxruntime, _ = runtime
    model_dir = resolve_model_dir(args)
    validate_model_dir(model_dir)
    providers = select_providers(onnxruntime, args.provider)
    encoder, decoder, joiner = load_sessions(model_dir, onnxruntime, providers)
    vocab = load_vocabulary(model_dir / "tokens.txt")
    metadata = read_streaming_metadata(encoder)
    mel_filterbank = create_mel_filterbank(numpy)
    return model_dir, encoder, decoder, joiner, vocab, metadata, mel_filterbank


def run_manifest(args: argparse.Namespace, runtime: tuple[Any, Any, Any, Any]) -> None:
    manifest_path = Path(args.manifest).resolve()
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    samples = manifest.get("samples")
    if not isinstance(samples, list):
        fail("Manifest must include samples[]", 2)

    loaded_model = load_model(args, runtime)
    runs = []
    for sample in samples:
        sample_id = sample.get("id") if isinstance(sample, dict) else None
        audio = sample.get("audio") if isinstance(sample, dict) else None
        if not sample_id or not audio:
            fail("Each manifest sample must include id and audio", 2)

        audio_path = (manifest_path.parent / audio).resolve()
        started = time.perf_counter()
        text = transcribe_loaded(args, runtime, loaded_model, str(audio_path))
        latency_ms = round((time.perf_counter() - started) * 1000)
        runs.append(
            {
                "sampleId": sample_id,
                "hypothesis": text,
                "latencyMs": latency_ms,
            }
        )

    output_path = Path(args.output).resolve()
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(
        json.dumps({"runs": runs}, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )


def main() -> int:
    args = parse_args()
    runtime = import_runtime()
    _, _, onnxruntime, _ = runtime
    model_dir = resolve_model_dir(args)
    validate_model_dir(model_dir)
    providers = select_providers(onnxruntime, args.provider)

    if args.preflight:
        load_sessions(model_dir, onnxruntime, providers)
        print(f"ok: Nemotron streaming ONNX available at {model_dir}")
        return 0

    if args.manifest:
        run_manifest(args, runtime)
        return 0

    loaded_model = load_model(args, runtime)
    text = transcribe_loaded(args, runtime, loaded_model, args.audio)
    if args.json:
        print(json.dumps({"text": text}, ensure_ascii=False))
    else:
        print(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
