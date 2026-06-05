# Release Smoke Checklist

Use this checklist after a release workflow succeeds or when validating a new
download path from the landing page. Record the command output, release URL, and
asset SHA-256 values in the project kanban.

## Automated Release Checks

Run from the app repo root:

```sh
pnpm release:verify
pnpm release:verify:macos
pnpm release:verify:windows
```

Expected results:

- Latest GitHub release tag matches `package.json`.
- Public assets are exactly `dybur-macos-arm64.dmg` and `dybur-windows-x64.exe`.
- Legacy public asset names are absent.
- `/latest/download/` URLs resolve.
- macOS DMG and Windows EXE download, meet size floors, and print SHA-256 values.

## macOS Manual Smoke

Run on an Apple Silicon Mac:

```sh
node scripts/verify-macos-release.js --require-macos-checks
```

Then install and check:

1. Mount the downloaded DMG.
2. Drag dybur into Applications.
3. Launch dybur from Applications, not the mounted image.
4. Confirm Gatekeeper allows launch and the app identity looks expected.
5. Grant microphone and Accessibility permissions.
6. Focus a normal text field, record a short dictation, and confirm text is inserted.
7. Focus a password field and confirm dybur does not inject dictated text.
8. Restart the app and confirm settings, hotkey, model selection, and tray controls persist.

## Windows Manual Smoke

Run on Windows 10 or 11:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\verify-windows-release.ps1
```

If Windows signing secrets are configured for the release workflow, require a
valid signature:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\verify-windows-release.ps1 -RequireSignature
```

Then install/run and check:

1. Download `dybur-windows-x64.exe` from the latest release.
2. Launch it from a normal user account.
3. Note SmartScreen or publisher prompts. Unsigned builds are expected to warn.
4. Confirm the tray icon appears.
5. Focus a normal text field, record a short dictation, and confirm insertion.
6. Focus a password field and confirm dybur does not inject dictated text.
7. Restart the app and confirm settings, hotkey, model selection, and tray controls persist.

## Landing Page Checks

Run from the landing repo:

```sh
pnpm verify:site
pnpm verify:downloads
```

After the Cloudflare DNS record exists, also run:

```sh
pnpm verify:site -- --require-www
```

`www.dybur.com` should be a proxied CNAME to `dybur-web.pages.dev` and should
redirect to `https://dybur.com`.

## ASR Candidate Checks

Before adding any experimental model to the production registry:

1. Record a fixed local corpus with language, noise, length, and domain tags.
2. Validate the corpus:

```sh
pnpm eval:asr:manifest benchmarks/asr/<run>.json --config benchmarks/asr/corpus-policy.example.json
```

3. Preflight enabled local runtimes:

```sh
pnpm eval:asr:candidates --commands benchmarks/asr/candidate-commands.local.json --preflight
```

4. Run, score, and gate candidates:

```sh
pnpm eval:asr:candidates benchmarks/asr/<run>.json \
  --commands benchmarks/asr/candidate-commands.local.json \
  --output benchmarks/asr/candidate-runs.json

pnpm eval:asr benchmarks/asr/candidate-runs.json \
  --format json \
  --output benchmarks/asr/candidate-report.json \
  --strict

pnpm eval:asr:gate benchmarks/asr/candidate-report.json \
  --config benchmarks/asr/gates/candidate-promotion.example.json
```

Keep CoreML Parakeet as the preferred Apple Silicon spike until it has a clean
Mac adapter, signed build smoke, and WER/latency parity or better versus the
current ONNX Parakeet default.
