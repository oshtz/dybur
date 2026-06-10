#!/usr/bin/env python3
"""Benchmark wrapper for dybur's installed Parakeet TDT ONNX models.

This script intentionally uses the same local model files that dybur installs
under ~/.dybur/models. It prints only the transcript by default so
scripts/asr-candidate-runner.js can score the production baseline with the ASR
evaluation harness.
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path
from typing import Any


DEFAULT_MODEL_ID = "parakeet-tdt-v3-int8"
REQUIRED_FILES = (
    "encoder-model.int8.onnx",
    "decoder_joint-model.int8.onnx",
    "nemo128.onnx",
    "vocab.txt",
)
WORD_BOUNDARY = "\u2581"


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
            + ". Create an isolated benchmark env and install `onnxruntime`, "
            "`soundfile`, and `librosa`.",
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
        description="Run an installed Parakeet TDT ONNX model on one audio file."
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
        default=5,
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
    if sample_rate != 16000:
        audio = librosa.resample(audio, orig_sr=sample_rate, target_sr=16000)
    return audio.astype(numpy.float32)


def load_sessions(model_dir: Path, onnxruntime: Any, providers: list[str]) -> tuple[Any, Any, Any]:
    session_options = onnxruntime.SessionOptions()
    preprocessor = onnxruntime.InferenceSession(
        str(model_dir / "nemo128.onnx"),
        sess_options=session_options,
        providers=providers,
    )
    encoder = onnxruntime.InferenceSession(
        str(model_dir / "encoder-model.int8.onnx"),
        sess_options=session_options,
        providers=providers,
    )
    decoder = onnxruntime.InferenceSession(
        str(model_dir / "decoder_joint-model.int8.onnx"),
        sess_options=session_options,
        providers=providers,
    )
    return preprocessor, encoder, decoder


def load_model(
    args: argparse.Namespace,
    runtime: tuple[Any, Any, Any, Any],
) -> tuple[Path, Any, Any, Any, list[str]]:
    _, _, onnxruntime, _ = runtime
    model_dir = resolve_model_dir(args)
    validate_model_dir(model_dir)
    providers = select_providers(onnxruntime, args.provider)
    preprocessor, encoder, decoder = load_sessions(model_dir, onnxruntime, providers)
    vocab = load_vocabulary(model_dir / "vocab.txt")
    return model_dir, preprocessor, encoder, decoder, vocab


def has_repeating_pattern(tokens: list[int], window: int = 10) -> bool:
    recent = tokens[-window:]
    if len(recent) < 6:
        return False

    half = len(recent) // 2
    for pattern_len in range(2, half + 1):
        if recent[-pattern_len:] == recent[-2 * pattern_len : -pattern_len]:
            return True
    return False


def greedy_tdt_decode(
    args: argparse.Namespace,
    encoded: Any,
    encoded_lengths: Any,
    runtime: tuple[Any, Any, Any, Any],
    vocab: list[str],
    decoder: Any,
) -> list[int]:
    _, numpy, _, _ = runtime
    blank_id = find_token_id(vocab, "<blk>")
    if blank_id is None:
        blank_id = 8192

    encoded_time_major = numpy.transpose(encoded, (0, 2, 1))
    encoder_time = int(encoded_lengths[0]) if len(encoded_lengths) > 0 else encoded_time_major.shape[1]
    encoder_time = min(encoder_time, encoded_time_major.shape[1])

    state_1 = numpy.zeros((2, 1, 640), dtype=numpy.float32)
    state_2 = numpy.zeros((2, 1, 640), dtype=numpy.float32)
    prev_token = numpy.array([[blank_id]], dtype=numpy.int32)
    target_length = numpy.array([1], dtype=numpy.int32)
    tokens: list[int] = []
    t = 0

    while t < encoder_time and len(tokens) < args.max_total_tokens:
        symbols_this_step = 0

        while True:
            encoder_frame = numpy.transpose(encoded_time_major[:, t : t + 1, :], (0, 2, 1)).copy()
            outputs = decoder.run(
                None,
                {
                    "encoder_outputs": encoder_frame.astype(numpy.float32),
                    "targets": prev_token,
                    "target_length": target_length,
                    "input_states_1": state_1,
                    "input_states_2": state_2,
                },
            )

            logits = outputs[0].reshape(-1)
            new_state_1 = outputs[2]
            new_state_2 = outputs[3]
            vocab_logits = logits[: len(vocab)]
            duration_logits = logits[len(vocab) :]
            best_token = int(numpy.argmax(vocab_logits))
            best_duration = int(numpy.argmax(duration_logits)) if len(duration_logits) else 1

            if best_token == blank_id:
                t += 1
                break

            state_1 = new_state_1
            state_2 = new_state_2
            tokens.append(best_token)
            prev_token = numpy.array([[best_token]], dtype=numpy.int32)
            symbols_this_step += 1

            if has_repeating_pattern(tokens):
                t += 1
                break

            if symbols_this_step >= args.max_symbols_per_step:
                t += max(best_duration, 1)
                break

            if best_duration > 0:
                t += best_duration
                break

    return tokens


def transcribe_loaded(
    args: argparse.Namespace,
    runtime: tuple[Any, Any, Any, Any],
    loaded_model: tuple[Path, Any, Any, Any, list[str]],
    audio_path: str,
) -> str:
    librosa, numpy, _, soundfile = runtime
    _, preprocessor, encoder, decoder, vocab = loaded_model
    audio = load_audio(audio_path, librosa, numpy, soundfile)

    features, feature_lengths = preprocessor.run(
        None,
        {
            "waveforms": audio.reshape(1, -1),
            "waveforms_lens": numpy.array([len(audio)], dtype=numpy.int64),
        },
    )
    encoded, encoded_lengths = encoder.run(
        None,
        {
            "audio_signal": features,
            "length": feature_lengths.astype(numpy.int64),
        },
    )
    tokens = greedy_tdt_decode(args, encoded, encoded_lengths, runtime, vocab, decoder)
    return decode_tokens(tokens, vocab)


def transcribe_audio(args: argparse.Namespace, runtime: tuple[Any, Any, Any, Any]) -> str:
    loaded_model = load_model(args, runtime)
    return transcribe_loaded(args, runtime, loaded_model, args.audio)


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
        print(f"ok: Parakeet ONNX baseline available at {model_dir}")
        return 0

    if args.manifest:
        run_manifest(args, runtime)
        return 0

    text = transcribe_audio(args, runtime)
    if args.json:
        print(json.dumps({"text": text}, ensure_ascii=False))
    else:
        print(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
