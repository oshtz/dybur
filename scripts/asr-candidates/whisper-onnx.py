#!/usr/bin/env python3
"""Benchmark wrapper for dybur's installed Whisper ONNX models."""

from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path
from typing import Any


DEFAULT_MODEL_ID = "whisper-large-v3-turbo-int8"
SAMPLE_RATE = 16000
WHISPER_SOT = 50258
WHISPER_EOT = 50257
WHISPER_EN = 50259
WHISPER_TRANSCRIBE = 50359
WHISPER_NO_TIMESTAMPS = 50363
TIMESTAMP_BEGIN = 50364


def fail(message: str, exit_code: int = 1) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(exit_code)


def import_runtime() -> tuple[Any, Any, Any, Any, Any, Any]:
    missing = []
    modules = {}

    for module_name in ("librosa", "numpy", "onnxruntime", "soundfile"):
        try:
            modules[module_name] = __import__(module_name)
        except ImportError:
            missing.append(module_name)

    try:
        from transformers import WhisperFeatureExtractor, WhisperTokenizerFast
    except ImportError:
        missing.append("transformers")
        WhisperFeatureExtractor = None
        WhisperTokenizerFast = None

    if missing:
        fail(
            "Missing dependency: "
            + ", ".join(missing)
            + ". Install `onnxruntime`, `soundfile`, `librosa`, and `transformers` "
            "in the isolated benchmark Python environment.",
            2,
        )

    return (
        modules["librosa"],
        modules["numpy"],
        modules["onnxruntime"],
        modules["soundfile"],
        WhisperFeatureExtractor,
        WhisperTokenizerFast,
    )


def default_model_dir(model_id: str) -> Path:
    return Path.home() / ".dybur" / "models" / model_id


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run an installed Whisper ONNX model on one audio file."
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
        "--precision",
        choices=("auto", "int8", "fp16"),
        default="auto",
        help="Whisper ONNX precision to load.",
    )
    parser.add_argument(
        "--provider",
        choices=("auto", "cpu", "cuda", "dml"),
        default="auto",
        help="ONNX Runtime execution provider.",
    )
    parser.add_argument(
        "--disable-optimizations",
        action="store_true",
        help="Disable ONNX graph optimizations. Required for this FP16 export on ORT CPU.",
    )
    parser.add_argument(
        "--language",
        default="en",
        help="Whisper language token to force, without angle brackets. Default: en.",
    )
    parser.add_argument(
        "--max-new-tokens",
        type=int,
        default=96,
        help="Maximum generated text tokens per utterance.",
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
    if args.max_new_tokens <= 0:
        parser.error("--max-new-tokens must be positive")
    return args


def resolve_model_dir(args: argparse.Namespace) -> Path:
    model_dir = Path(args.model_dir).expanduser() if args.model_dir else default_model_dir(args.model_id)
    return model_dir.resolve()


def resolve_precision(args: argparse.Namespace, model_dir: Path) -> str:
    if args.precision != "auto":
        return args.precision
    if "fp16" in args.model_id:
        return "fp16"
    if "int8" in args.model_id:
        return "int8"
    if (model_dir / "onnx" / "encoder_model_int8.onnx").exists():
        return "int8"
    if (model_dir / "onnx" / "encoder_model_fp16.onnx").exists():
        return "fp16"
    return "int8"


def model_files(model_dir: Path, precision: str) -> tuple[Path, Path]:
    return (
        model_dir / "onnx" / f"encoder_model_{precision}.onnx",
        model_dir / "onnx" / f"decoder_model_{precision}.onnx",
    )


def validate_model_dir(model_dir: Path, precision: str) -> tuple[Path, Path]:
    if not model_dir.exists():
        fail(f"Model directory not found: {model_dir}", 2)

    encoder_path, decoder_path = model_files(model_dir, precision)
    required = [encoder_path, decoder_path, model_dir / "tokenizer.json", model_dir / "config.json"]
    missing = [str(path.relative_to(model_dir)) for path in required if not path.exists()]
    if missing:
        fail(f"Model directory is missing required file(s): {', '.join(missing)}", 2)
    return encoder_path, decoder_path


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


def session_options(onnxruntime: Any, disable_optimizations: bool) -> Any:
    options = onnxruntime.SessionOptions()
    if disable_optimizations:
        options.graph_optimization_level = onnxruntime.GraphOptimizationLevel.ORT_DISABLE_ALL
    return options


def load_sessions(
    encoder_path: Path,
    decoder_path: Path,
    onnxruntime: Any,
    providers: list[str],
    disable_optimizations: bool,
) -> tuple[Any, Any]:
    options = session_options(onnxruntime, disable_optimizations)
    encoder = onnxruntime.InferenceSession(str(encoder_path), sess_options=options, providers=providers)
    decoder = onnxruntime.InferenceSession(str(decoder_path), sess_options=options, providers=providers)
    return encoder, decoder


def load_audio(audio_path: str, librosa: Any, numpy: Any, soundfile: Any) -> Any:
    audio, sample_rate = soundfile.read(audio_path, dtype="float32")
    if getattr(audio, "ndim", 1) > 1:
        audio = audio.mean(axis=1)
    if sample_rate != SAMPLE_RATE:
        audio = librosa.resample(audio, orig_sr=sample_rate, target_sr=SAMPLE_RATE)
    return audio.astype(numpy.float32)


def tokenizer_id(tokenizer: Any, token: str, fallback: int) -> int:
    token_id = tokenizer.convert_tokens_to_ids(token)
    if token_id is None or token_id == tokenizer.unk_token_id:
        return fallback
    return int(token_id)


def build_prompt(tokenizer: Any, language: str) -> list[int]:
    language_token = tokenizer_id(tokenizer, f"<|{language}|>", WHISPER_EN)
    return [
        tokenizer_id(tokenizer, "<|startoftranscript|>", WHISPER_SOT),
        language_token,
        tokenizer_id(tokenizer, "<|transcribe|>", WHISPER_TRANSCRIBE),
        tokenizer_id(tokenizer, "<|notimestamps|>", WHISPER_NO_TIMESTAMPS),
    ]


def run_encoder(encoder: Any, input_features: Any) -> Any:
    input_name = encoder.get_inputs()[0].name
    return encoder.run(None, {input_name: input_features})[0]


def run_decoder(
    args: argparse.Namespace,
    runtime: tuple[Any, Any, Any, Any, Any, Any],
    decoder: Any,
    tokenizer: Any,
    encoder_hidden_states: Any,
) -> list[int]:
    _, numpy, _, _, _, _ = runtime
    token_input_name = "input_ids"
    encoder_input_name = "encoder_hidden_states"
    for input_meta in decoder.get_inputs():
        name = input_meta.name
        lower = name.lower()
        if "input_id" in lower or lower == "tokens":
            token_input_name = name
        if "encoder" in lower and "hidden" in lower:
            encoder_input_name = name

    eot_token_id = tokenizer.eos_token_id or WHISPER_EOT
    tokens = build_prompt(tokenizer, args.language)
    generated: list[int] = []

    for _ in range(args.max_new_tokens):
        decoder_inputs = {
            token_input_name: numpy.array([tokens], dtype=numpy.int64),
            encoder_input_name: encoder_hidden_states,
        }
        outputs = decoder.run(None, decoder_inputs)
        logits = outputs[0][0, -1].copy()
        if len(logits) > TIMESTAMP_BEGIN:
            logits[TIMESTAMP_BEGIN:] = -float("inf")
        next_token = int(numpy.argmax(logits))
        if next_token == eot_token_id:
            break
        tokens.append(next_token)
        generated.append(next_token)

    return generated


def load_model(
    args: argparse.Namespace,
    runtime: tuple[Any, Any, Any, Any, Any, Any],
) -> tuple[Path, str, Any, Any, Any, Any]:
    _, _, onnxruntime, _, WhisperFeatureExtractor, WhisperTokenizerFast = runtime
    model_dir = resolve_model_dir(args)
    precision = resolve_precision(args, model_dir)
    encoder_path, decoder_path = validate_model_dir(model_dir, precision)
    providers = select_providers(onnxruntime, args.provider)
    feature_extractor = WhisperFeatureExtractor(
        feature_size=128,
        sampling_rate=SAMPLE_RATE,
        hop_length=160,
        chunk_length=30,
        n_fft=400,
    )
    tokenizer = WhisperTokenizerFast.from_pretrained(model_dir)
    encoder, decoder = load_sessions(
        encoder_path,
        decoder_path,
        onnxruntime,
        providers,
        args.disable_optimizations,
    )
    return model_dir, precision, feature_extractor, tokenizer, encoder, decoder


def transcribe_loaded(
    args: argparse.Namespace,
    runtime: tuple[Any, Any, Any, Any, Any, Any],
    loaded_model: tuple[Path, str, Any, Any, Any, Any],
    audio_path: str,
) -> str:
    librosa, numpy, _, soundfile, _, _ = runtime
    _, _, feature_extractor, tokenizer, encoder, decoder = loaded_model
    audio = load_audio(audio_path, librosa, numpy, soundfile)
    features = feature_extractor(audio, sampling_rate=SAMPLE_RATE, return_tensors="np").input_features
    encoder_hidden_states = run_encoder(encoder, features.astype(numpy.float32))
    tokens = run_decoder(args, runtime, decoder, tokenizer, encoder_hidden_states)
    return tokenizer.decode(tokens, skip_special_tokens=True).strip()


def run_manifest(args: argparse.Namespace, runtime: tuple[Any, Any, Any, Any, Any, Any]) -> None:
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
    _, _, onnxruntime, _, _, _ = runtime
    model_dir = resolve_model_dir(args)
    precision = resolve_precision(args, model_dir)
    encoder_path, decoder_path = validate_model_dir(model_dir, precision)
    providers = select_providers(onnxruntime, args.provider)

    if args.preflight:
        load_sessions(
            encoder_path,
            decoder_path,
            onnxruntime,
            providers,
            args.disable_optimizations,
        )
        print(f"ok: Whisper ONNX {precision} available at {model_dir}")
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
