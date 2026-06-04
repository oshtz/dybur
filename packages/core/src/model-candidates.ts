/**
 * Experimental model candidates for future dybur support.
 *
 * These are intentionally separate from MODEL_REGISTRY. Entries here are not
 * selectable production models until dybur has a verified local runtime adapter
 * for the candidate.
 */

export type CandidateRuntime =
  | 'coreml'
  | 'mlx'
  | 'python_transformers'
  | 'python_mlx'
  | 'vllm'
  | 'nemo';

export type CandidateRecommendation = 'recommended' | 'benchmark' | 'defer';

export interface ModelCandidate {
  id: string;
  displayName: string;
  provider: string;
  sourceUrl: string;
  runtime: CandidateRuntime;
  platforms: string[];
  license: string;
  sizeLabel: string;
  languageLabel: string;
  recommendation: CandidateRecommendation;
  rationale: string;
  integrationRisk: string;
  nextStep: string;
  benchmarkHint?: string;
}

export const MODEL_CANDIDATES: ModelCandidate[] = [
  {
    id: 'parakeet-tdt-v3-coreml',
    displayName: 'Parakeet TDT v3 CoreML (Apple Silicon)',
    provider: 'FluidInference',
    sourceUrl: 'https://huggingface.co/FluidInference/parakeet-tdt-0.6b-v3-coreml',
    runtime: 'coreml',
    platforms: ['darwin-arm64'],
    license: 'CC-BY-4.0',
    sizeLabel: '1.76 GB',
    languageLabel: '25 European languages',
    recommendation: 'recommended',
    rationale:
      'Highest value macOS spike: smaller than the MLX Parakeet bundle and aligned with native Apple acceleration.',
    integrationRisk:
      'Requires a macOS-only CoreML adapter or Swift sidecar; it is not compatible with the current ONNX-only Rust STT path.',
    nextStep:
      'Prototype a macOS adapter that invokes the CoreML package, then compare WER and latency against parakeet-tdt-v3-int8.',
    benchmarkHint: 'node scripts/asr-candidates/fluidaudio-coreml.js "{audio}"',
  },
  {
    id: 'parakeet-tdt-v3-mlx',
    displayName: 'Parakeet TDT v3 MLX',
    provider: 'mlx-community',
    sourceUrl: 'https://huggingface.co/mlx-community/parakeet-tdt-0.6b-v3',
    runtime: 'python_mlx',
    platforms: ['darwin-arm64'],
    license: 'CC-BY-4.0',
    sizeLabel: '2.51 GB',
    languageLabel: '25 European languages',
    recommendation: 'benchmark',
    rationale:
      'Useful Apple Silicon benchmark target for Parakeet v3 before deciding whether MLX belongs in the product runtime.',
    integrationRisk:
      'Large download and Python/MLX dependency chain make it a poor production default without a native adapter.',
    nextStep:
      'Run external benchmark commands with the ASR candidate runner and compare against ONNX and CoreML Parakeet.',
    benchmarkHint: 'parakeet-mlx "{audio}" --model mlx-community/parakeet-tdt-0.6b-v3',
  },
  {
    id: 'qwen3-asr-0.6b',
    displayName: 'Qwen3-ASR 0.6B',
    provider: 'Qwen',
    sourceUrl: 'https://huggingface.co/Qwen/Qwen3-ASR-0.6B',
    runtime: 'python_transformers',
    platforms: ['darwin-arm64', 'windows-x64', 'linux-x64'],
    license: 'Apache-2.0',
    sizeLabel: '0.6B parameters',
    languageLabel: '52 languages and dialects',
    recommendation: 'benchmark',
    rationale:
      'Best candidate for broader multilingual coverage and unified offline/streaming behavior beyond Parakeet v3.',
    integrationRisk:
      'Current dybur runtime cannot load this architecture directly; it needs a separate Transformers, MLX, or future ONNX adapter.',
    nextStep:
      'Benchmark accuracy, latency, and memory on the same corpus before exposing it as an experimental model option.',
    benchmarkHint:
      'python scripts/asr-candidates/qwen3-asr.py "{audio}" --model Qwen/Qwen3-ASR-0.6B',
  },
  {
    id: 'moonshine-streaming-tiny',
    displayName: 'Moonshine Streaming Tiny',
    provider: 'Useful Sensors',
    sourceUrl: 'https://huggingface.co/UsefulSensors/moonshine-streaming-tiny',
    runtime: 'python_transformers',
    platforms: ['darwin-arm64', 'windows-x64', 'linux-x64'],
    license: 'MIT',
    sizeLabel: '44.1M parameters',
    languageLabel: 'English',
    recommendation: 'benchmark',
    rationale:
      'Promising low-latency English option for lightweight dictation and constrained devices.',
    integrationRisk:
      'Model card notes that the Transformers path is not fully efficient streaming yet; production value depends on a better runtime path.',
    nextStep:
      'Benchmark short dictation latency and hallucination behavior against Nemotron streaming and Parakeet.',
    benchmarkHint:
      'python scripts/asr-candidates/moonshine-transformers.py "{audio}" --model UsefulSensors/moonshine-streaming-tiny',
  },
  {
    id: 'canary-1b-v2',
    displayName: 'Canary 1B v2',
    provider: 'NVIDIA',
    sourceUrl: 'https://huggingface.co/nvidia/canary-1b-v2',
    runtime: 'nemo',
    platforms: ['linux-x64'],
    license: 'CC-BY-4.0',
    sizeLabel: '1B parameters',
    languageLabel: '25 European languages plus speech translation',
    recommendation: 'defer',
    rationale:
      'Strong ASR/translation model, but dybur is a dictation app and this is less runtime-compatible than Parakeet/Qwen/Moonshine.',
    integrationRisk:
      'NeMo/PyTorch/Linux/CUDA orientation does not fit dybur desktop packaging today.',
    nextStep: 'Revisit only if speech translation becomes a product goal.',
  },
  {
    id: 'voxtral-mini-3b',
    displayName: 'Voxtral Mini 3B',
    provider: 'Mistral AI',
    sourceUrl: 'https://huggingface.co/mistralai/Voxtral-Mini-3B-2507',
    runtime: 'vllm',
    platforms: ['linux-x64'],
    license: 'Apache-2.0',
    sizeLabel:
      'Mini 3B label; Hugging Face lists 5B params and roughly 9.5 GB GPU RAM in fp16/bf16',
    languageLabel: '8 major languages',
    recommendation: 'defer',
    rationale:
      'Interesting for audio understanding, summaries, and voice-command workflows, not for lightweight plain dictation.',
    integrationRisk:
      'Heavy vLLM/GPU runtime and broader speech-understanding behavior would expand the product surface substantially.',
    nextStep:
      'Revisit only if dybur grows beyond text insertion into audio-understanding workflows.',
  },
];

export function getModelCandidates(options: { includeDeferred?: boolean } = {}): ModelCandidate[] {
  return MODEL_CANDIDATES.filter(
    (candidate) => options.includeDeferred || candidate.recommendation !== 'defer'
  );
}

export function getModelCandidate(id: string): ModelCandidate | undefined {
  return MODEL_CANDIDATES.find((candidate) => candidate.id === id);
}
