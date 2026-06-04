import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, it } from 'node:test';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const gatePath = path.join(repoRoot, 'scripts', 'asr-gate.js');
const promotionConfigPath = path.join(
  repoRoot,
  'benchmarks',
  'asr',
  'gates',
  'candidate-promotion.example.json'
);

function makeTempReport(t, report) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'dybur-asr-gate-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));

  const reportPath = path.join(dir, 'report.json');
  fs.writeFileSync(reportPath, `${JSON.stringify(report, null, 2)}\n`);
  return reportPath;
}

function makeTempConfig(t, config) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'dybur-asr-gate-config-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));

  const configPath = path.join(dir, 'gate.json');
  fs.writeFileSync(configPath, `${JSON.stringify(config, null, 2)}\n`);
  return configPath;
}

function runGate(args) {
  return spawnSync(process.execPath, [gatePath, ...args], {
    cwd: repoRoot,
    encoding: 'utf8',
    windowsHide: true,
  });
}

const report = {
  models: [
    {
      model: 'baseline',
      samples: 2,
      wer: 0.04,
      cer: 0.015,
      medianLatencyMs: 900,
      medianRealtimeFactor: 0.3,
    },
    {
      model: 'candidate',
      samples: 2,
      wer: 0.05,
      cer: 0.018,
      medianLatencyMs: 880,
      medianRealtimeFactor: 0.28,
    },
  ],
  tagSummaries: [
    {
      tag: 'noisy',
      model: 'baseline',
      samples: 1,
      wer: 0.06,
      cer: 0.02,
      medianLatencyMs: 900,
      medianRealtimeFactor: 0.3,
    },
    {
      tag: 'noisy',
      model: 'candidate',
      samples: 1,
      wer: 0.07,
      cer: 0.025,
      medianLatencyMs: 880,
      medianRealtimeFactor: 0.28,
    },
  ],
};

const promotionReport = {
  models: [
    {
      model: 'parakeet-tdt-v3-int8',
      samples: 2,
      wer: 0.04,
      cer: 0.015,
      medianLatencyMs: 900,
      medianRealtimeFactor: 0.3,
    },
    {
      model: 'candidate',
      samples: 2,
      wer: 0.05,
      cer: 0.018,
      medianLatencyMs: 950,
      medianRealtimeFactor: 0.35,
    },
  ],
  tagSummaries: [
    {
      tag: 'english',
      model: 'parakeet-tdt-v3-int8',
      samples: 2,
      wer: 0.04,
      cer: 0.015,
      medianLatencyMs: 900,
      medianRealtimeFactor: 0.3,
    },
    {
      tag: 'english',
      model: 'candidate',
      samples: 2,
      wer: 0.05,
      cer: 0.018,
      medianLatencyMs: 950,
      medianRealtimeFactor: 0.35,
    },
  ],
};

describe('asr-gate', () => {
  it('accepts the checked-in candidate promotion config', (t) => {
    const reportPath = makeTempReport(t, promotionReport);

    const result = runGate([reportPath, '--config', promotionConfigPath]);

    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /ASR gate passed for 1 candidate model\(s\)\./);
  });

  it('loads reusable gate config files', (t) => {
    const reportPath = makeTempReport(t, report);
    const configPath = makeTempConfig(t, {
      baseline: 'baseline',
      maxWer: 0.08,
      maxCer: 0.03,
      maxRtf: 0.4,
      maxWerRegression: 0.02,
    });

    const result = runGate([reportPath, '--config', configPath]);

    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /ASR gate passed for 1 candidate model\(s\)\./);
  });

  it('lets CLI flags override config thresholds', (t) => {
    const reportPath = makeTempReport(t, report);
    const configPath = makeTempConfig(t, {
      baseline: 'baseline',
      candidates: ['candidate'],
      maxWer: 0.04,
    });

    const result = runGate([reportPath, '--config', configPath, '--max-wer', '0.08']);

    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /ASR gate passed for 1 candidate model\(s\)\./);
  });

  it('passes when candidates stay within absolute and baseline thresholds', (t) => {
    const reportPath = makeTempReport(t, report);

    const result = runGate([
      reportPath,
      '--baseline',
      'baseline',
      '--candidate',
      'candidate',
      '--max-wer',
      '0.08',
      '--max-cer',
      '0.03',
      '--max-rtf',
      '0.4',
      '--max-wer-regression',
      '0.02',
    ]);

    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /ASR gate passed for 1 candidate model\(s\)\./);
  });

  it('fails absolute thresholds for overall summaries', (t) => {
    const reportPath = makeTempReport(t, report);

    const result = runGate([reportPath, '--candidate', 'candidate', '--max-wer', '0.04']);

    assert.equal(result.status, 1);
    assert.match(result.stderr, /model candidate wer 5\.0% exceeds 4\.0%/);
  });

  it('fails regression thresholds for tag summaries', (t) => {
    const reportPath = makeTempReport(t, report);

    const result = runGate([
      reportPath,
      '--baseline',
      'baseline',
      '--candidate',
      'candidate',
      '--max-wer-regression',
      '0.005',
    ]);

    assert.equal(result.status, 1);
    assert.match(result.stderr, /tag noisy model candidate wer regression 1\.0% exceeds 0\.5%/);
  });

  it('reports missing baseline and candidate models', (t) => {
    const reportPath = makeTempReport(t, report);

    const result = runGate([
      reportPath,
      '--baseline',
      'missing-baseline',
      '--candidate',
      'missing-candidate',
      '--max-wer-regression',
      '0.01',
    ]);

    assert.equal(result.status, 1);
    assert.match(result.stderr, /Baseline model not found: missing-baseline/);
    assert.match(result.stderr, /Candidate model not found: missing-candidate/);
  });
});
