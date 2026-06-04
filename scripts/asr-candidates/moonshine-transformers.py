#!/usr/bin/env python3
"""Benchmark wrapper for Moonshine Streaming via Transformers.

This is an offline candidate benchmark path, not a production dybur runtime.
It prints only the transcript by default so scripts/asr-candidate-runner.js can
feed its output into scripts/asr-eval.js.
"""

from __future__ import annotations

import argparse
import json
import sys
from typing import Any


DEFAULT_MODEL = "UsefulSensors/moonshine-streaming-tiny"


def fail(message: str, exit_code: int = 1) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(exit_code)


def import_runtime() -> tuple[Any, Any]:
    try:
        import torch
    except ImportError:
        fail(
            "Missing dependency: torch. Install a fresh benchmark environment with "
            "`pip install -U git+https://github.com/huggingface/transformers.git "
            "datasets[audio] torch`.",
            2,
        )

    try:
        from transformers import pipeline
    except ImportError:
        fail(
            "Missing dependency: transformers. Install the latest Transformers from git: "
            "`pip install -U git+https://github.com/huggingface/transformers.git "
            "datasets[audio] torch`.",
            2,
        )

    return torch, pipeline


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run Moonshine Streaming Tiny on one audio file and print the transcript."
    )
    parser.add_argument(
        "audio",
        nargs="?",
        help="Path or URL accepted by the Transformers ASR pipeline.",
    )
    parser.add_argument("--model", default=DEFAULT_MODEL, help=f"Model id. Default: {DEFAULT_MODEL}")
    parser.add_argument(
        "--device",
        choices=("auto", "cpu", "cuda", "mps"),
        default="auto",
        help="Execution device for the Transformers pipeline.",
    )
    parser.add_argument(
        "--dtype",
        choices=("auto", "float32", "float16"),
        default="auto",
        help="Torch dtype. Default: float16 on CUDA, otherwise float32.",
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
    if not args.preflight and not args.audio:
        parser.error("audio is required unless --preflight is used")
    return args


def resolve_device(torch: Any, requested: str) -> Any:
    if requested == "cpu":
        return -1
    if requested == "cuda":
        return 0
    if requested == "mps":
        return "mps"
    if torch.cuda.is_available():
        return 0
    if getattr(torch.backends, "mps", None) and torch.backends.mps.is_available():
        return "mps"
    return -1


def resolve_dtype(torch: Any, requested: str) -> Any:
    if requested == "float32":
        return torch.float32
    if requested == "float16":
        return torch.float16
    return torch.float16 if torch.cuda.is_available() else torch.float32


def main() -> int:
    args = parse_args()
    torch, pipeline = import_runtime()

    if args.preflight:
        print(f"ok: moonshine runtime available; torch {torch.__version__}")
        return 0

    recognizer = pipeline(
        "automatic-speech-recognition",
        model=args.model,
        device=resolve_device(torch, args.device),
        torch_dtype=resolve_dtype(torch, args.dtype),
    )

    result = recognizer(args.audio)
    text = result.get("text", "") if isinstance(result, dict) else str(result)

    if args.json:
        print(json.dumps({"text": text}, ensure_ascii=False))
    else:
        print(text)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
