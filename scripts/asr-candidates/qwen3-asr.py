#!/usr/bin/env python3
"""Benchmark wrapper for Qwen3-ASR.

This script is intentionally outside dybur's production runtime. It runs a
local Qwen3-ASR Python environment and prints only the transcript by default so
scripts/asr-candidate-runner.js can score it with the shared ASR harness.
"""

from __future__ import annotations

import argparse
import json
import sys
from typing import Any


DEFAULT_MODEL = "Qwen/Qwen3-ASR-0.6B"


def fail(message: str, exit_code: int = 1) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(exit_code)


def import_runtime() -> tuple[Any, Any]:
    try:
        import torch
    except ImportError:
        fail(
            "Missing dependency: torch. Create an isolated Python 3.12 env and run "
            "`pip install -U qwen-asr`.",
            2,
        )

    try:
        from qwen_asr import Qwen3ASRModel
    except ImportError:
        fail(
            "Missing dependency: qwen_asr. Install the official runtime with "
            "`pip install -U qwen-asr`.",
            2,
        )

    return torch, Qwen3ASRModel


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run Qwen3-ASR on one audio file and print the transcript."
    )
    parser.add_argument(
        "audio",
        nargs="?",
        help="Path, URL, or base64 audio accepted by qwen-asr.",
    )
    parser.add_argument("--model", default=DEFAULT_MODEL, help=f"Model id. Default: {DEFAULT_MODEL}")
    parser.add_argument(
        "--language",
        default=None,
        help='Optional language hint, e.g. "English". Omit for auto-detection.',
    )
    parser.add_argument(
        "--device-map",
        default="auto",
        help='Device map passed to Qwen3ASRModel. Default: auto -> "cuda:0" or "cpu".',
    )
    parser.add_argument(
        "--dtype",
        choices=("auto", "float32", "float16", "bfloat16"),
        default="auto",
        help="Torch dtype. Default: bfloat16 on CUDA, otherwise float32.",
    )
    parser.add_argument(
        "--max-new-tokens",
        type=int,
        default=256,
        help="Maximum generated tokens for one utterance.",
    )
    parser.add_argument(
        "--max-batch-size",
        type=int,
        default=1,
        help="Qwen max_inference_batch_size for this single-file wrapper.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help='Print {"text": "...", "language": "..."} instead of plain text.',
    )
    parser.add_argument(
        "--preflight",
        action="store_true",
        help="Check runtime imports without loading the model or transcribing audio.",
    )
    args = parser.parse_args()
    if not args.preflight and not args.audio:
        parser.error("audio is required unless --preflight is used")
    return args


def resolve_device_map(torch: Any, requested: str) -> str:
    if requested != "auto":
        return requested
    return "cuda:0" if torch.cuda.is_available() else "cpu"


def resolve_dtype(torch: Any, requested: str) -> Any:
    if requested == "float32":
        return torch.float32
    if requested == "float16":
        return torch.float16
    if requested == "bfloat16":
        return torch.bfloat16
    return torch.bfloat16 if torch.cuda.is_available() else torch.float32


def main() -> int:
    args = parse_args()
    torch, qwen_model = import_runtime()

    if args.preflight:
        print(f"ok: qwen-asr runtime available; torch {torch.__version__}")
        return 0

    device_map = resolve_device_map(torch, args.device_map)
    dtype = resolve_dtype(torch, args.dtype)

    model = qwen_model.from_pretrained(
        args.model,
        dtype=dtype,
        device_map=device_map,
        max_inference_batch_size=args.max_batch_size,
        max_new_tokens=args.max_new_tokens,
    )

    results = model.transcribe(audio=args.audio, language=args.language)
    if not results:
        fail("Qwen3-ASR returned no transcription results")

    result = results[0]
    text = getattr(result, "text", "")
    language = getattr(result, "language", None)

    if args.json:
        print(json.dumps({"text": text, "language": language}, ensure_ascii=False))
    else:
        print(text)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
