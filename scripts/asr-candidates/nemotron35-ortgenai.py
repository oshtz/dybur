#!/usr/bin/env python3
"""Benchmark wrapper for Nemotron 3.5 ASR ONNX INT4 via ONNX Runtime GenAI.

This script is intentionally outside dybur's production runtime. It runs a
local Python benchmark environment and prints only the transcript by default so
scripts/asr-candidate-runner.js can score it with the shared ASR harness.
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path
from typing import Any


DEFAULT_MODEL = "onnx-community/nemotron-3.5-asr-streaming-0.6b-onnx-int4"
DEFAULT_CHUNK_SAMPLES = 8960


def fail(message: str, exit_code: int = 1) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(exit_code)


def import_runtime() -> tuple[Any, Any, Any, Any, Any]:
    missing = []
    modules = {}

    for module_name in ("huggingface_hub", "librosa", "numpy", "onnxruntime_genai", "soundfile"):
        try:
            modules[module_name] = __import__(module_name)
        except ImportError:
            missing.append(module_name)

    if missing:
        fail(
            "Missing dependency: "
            + ", ".join(missing)
            + ". Create an isolated benchmark env and install `onnxruntime-genai`, "
            "`huggingface_hub`, `soundfile`, and `librosa`.",
            2,
        )

    return (
        modules["huggingface_hub"],
        modules["librosa"],
        modules["numpy"],
        modules["onnxruntime_genai"],
        modules["soundfile"],
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run Nemotron 3.5 ASR ONNX INT4 on one audio file and print the transcript."
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
        "--model",
        default=DEFAULT_MODEL,
        help=f"Hugging Face repo id or local model directory. Default: {DEFAULT_MODEL}",
    )
    parser.add_argument(
        "--provider",
        choices=("auto", "cpu", "cuda", "dml", "openvino"),
        default="auto",
        help="ONNX Runtime execution provider.",
    )
    parser.add_argument(
        "--chunk-samples",
        type=int,
        default=DEFAULT_CHUNK_SAMPLES,
        help=f"Streaming chunk size at 16 kHz. Default: {DEFAULT_CHUNK_SAMPLES}.",
    )
    parser.add_argument(
        "--use-vad",
        action="store_true",
        help="Use the model bundle's VAD processor instead of forcing every chunk through ASR.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help='Print {"text": "..."} instead of plain text.',
    )
    parser.add_argument(
        "--preflight",
        action="store_true",
        help="Check runtime imports without loading the model or transcribing audio.",
    )
    args = parser.parse_args()
    if bool(args.manifest) != bool(args.output):
        parser.error("--manifest and --output must be used together")
    if args.manifest and args.audio:
        parser.error("audio positional input cannot be combined with --manifest")
    if not args.preflight and not args.audio and not args.manifest:
        parser.error("audio or --manifest is required unless --preflight is used")
    if args.chunk_samples <= 0:
        parser.error("--chunk-samples must be positive")
    return args


def resolve_model_dir(model: str, huggingface_hub: Any) -> str:
    model_path = Path(model)
    if model_path.exists():
        return str(model_path.resolve())
    return huggingface_hub.snapshot_download(model)


def configure_provider(config: Any, provider: str) -> None:
    if provider == "auto":
        return

    provider_names = {
        "cpu": "CPUExecutionProvider",
        "cuda": "CUDAExecutionProvider",
        "dml": "DmlExecutionProvider",
        "openvino": "OpenVINOExecutionProvider",
    }
    if hasattr(config, "clear_providers"):
        config.clear_providers()
    config.append_provider(provider_names[provider])


def load_audio(audio_path: str, librosa: Any, numpy: Any, soundfile: Any) -> Any:
    audio, sample_rate = soundfile.read(audio_path, dtype="float32")
    if getattr(audio, "ndim", 1) > 1:
        audio = audio.mean(axis=1)
    if sample_rate != 16000:
        audio = librosa.resample(audio, orig_sr=sample_rate, target_sr=16000)
    return audio.astype(numpy.float32)


def drain_generator(generator: Any, token_stream: Any) -> str:
    text = ""
    while not generator.is_done():
        generator.generate_next_token()
        tokens = generator.get_next_tokens()
        if len(tokens) > 0:
            chunk = token_stream.decode(tokens[0])
            if chunk:
                text += chunk
    return text


def load_model(args: argparse.Namespace, runtime: tuple[Any, Any, Any, Any, Any]) -> tuple[Any, Any]:
    huggingface_hub, _, _, og, _ = runtime
    model_dir = resolve_model_dir(args.model, huggingface_hub)
    config = og.Config(model_dir)
    configure_provider(config, args.provider)
    model = og.Model(config)
    tokenizer = og.Tokenizer(model)
    return model, tokenizer


def transcribe_loaded(
    args: argparse.Namespace,
    runtime: tuple[Any, Any, Any, Any, Any],
    loaded_model: tuple[Any, Any],
    audio_path: str,
) -> str:
    _, librosa, numpy, og, soundfile = runtime
    model, tokenizer = loaded_model
    audio = load_audio(audio_path, librosa, numpy, soundfile)
    params = og.GeneratorParams(model)
    processor = og.StreamingProcessor(model)
    processor.set_option("use_vad", "true" if args.use_vad else "false")
    generator = og.Generator(model, params)
    token_stream = tokenizer.create_stream()

    text = ""
    for start in range(0, len(audio), args.chunk_samples):
        inputs = processor.process(audio[start : start + args.chunk_samples])
        if inputs is not None:
            generator.set_inputs(inputs)
            text += drain_generator(generator, token_stream)

    inputs = processor.flush()
    if inputs is not None:
        generator.set_inputs(inputs)
        text += drain_generator(generator, token_stream)

    return text.strip()


def transcribe_audio(args: argparse.Namespace, runtime: tuple[Any, Any, Any, Any, Any]) -> str:
    loaded_model = load_model(args, runtime)
    return transcribe_loaded(args, runtime, loaded_model, args.audio)


def run_manifest(args: argparse.Namespace, runtime: tuple[Any, Any, Any, Any, Any]) -> None:
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

    if args.preflight:
        _, _, _, og, _ = runtime
        print(f"ok: ONNX Runtime GenAI available; version {getattr(og, '__version__', 'unknown')}")
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
