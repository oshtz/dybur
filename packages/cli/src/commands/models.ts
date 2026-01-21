/**
 * Models command - manage speech recognition models
 */

import {
  listModels,
  downloadModel,
  cleanModels,
  DEFAULT_MODEL,
  MODEL_REPO,
  isDefaultModelInstalled,
} from '@dybur/core';
import { getModelsDir } from '@dybur/config';
import {
  header,
  success,
  info,
  error,
  command,
  divider,
  brand,
  cyan,
  dim,
  green,
  formatSize,
  formatPath,
  progressBar,
  Spinner,
  icons,
} from '../ui.js';

function showModelsHelp(): void {
  header('Model Management');

  console.log(`  ${dim('dybur uses NVIDIA Parakeet for speech recognition.')}`);
  console.log(`  ${dim('Models are downloaded from HuggingFace on first use.')}`);
  console.log('');

  divider();
  console.log('');

  console.log(`  ${brand.accent('Commands')}`);
  command('models list', 'List installed models');
  command('models prefetch', 'Download default model');
  command('models clean', 'Remove unused models');
  console.log('');

  console.log(`  ${brand.accent('Default Model')}`);
  console.log(`  ${dim('Name:')}   ${DEFAULT_MODEL}`);
  console.log(`  ${dim('Source:')} huggingface.co/${MODEL_REPO}`);
  console.log(`  ${dim('Size:')}   ~670 MB (INT8 quantized)`);
  console.log('');
}

/**
 * List installed models
 */
async function listCommand(): Promise<void> {
  header('Installed Models');

  const models = listModels();
  const modelsDir = getModelsDir();

  if (models.length === 0) {
    info('No models installed');
    console.log('');
    console.log(`  ${dim('To install the default model:')}`);
    console.log(`  ${cyan('dybur models prefetch')}`);
    console.log('');
    console.log(`  ${dim('Models directory:')} ${formatPath(modelsDir, 45)}`);
    console.log('');
    return;
  }

  for (const model of models) {
    const defaultBadge = model.isDefault ? ` ${green('[default]')}` : '';
    const size = formatSize(model.size);

    console.log(`  ${brand.accent(icons.bullet)} ${model.name}${defaultBadge}`);
    console.log(`    ${dim('Size:')} ${size}`);

    if (model.metadata) {
      console.log(`    ${dim('Downloaded:')} ${model.metadata.downloadedAt.split('T')[0]}`);
      if (model.metadata.variant) {
        console.log(`    ${dim('Variant:')} ${model.metadata.variant}`);
      }
      if (model.metadata.source) {
        console.log(`    ${dim('Source:')} ${model.metadata.source}`);
      }
    }

    console.log('');
  }

  divider();
  console.log('');
  console.log(`  ${dim('Models directory:')} ${formatPath(modelsDir, 45)}`);
  console.log('');
}

/**
 * Download the default model
 */
async function prefetchCommand(): Promise<void> {
  header('Download Model');

  if (isDefaultModelInstalled()) {
    success(`Model already installed: ${DEFAULT_MODEL}`);
    console.log('');
    return;
  }

  console.log(`  ${dim('Model:')}  ${DEFAULT_MODEL}`);
  console.log(`  ${dim('Source:')} huggingface.co/${MODEL_REPO}`);
  console.log(`  ${dim('Variant:')} INT8 quantized (~670 MB)`);
  console.log('');

  divider();
  console.log('');

  let currentFile = '';
  let fileCount = 0;

  try {
    await downloadModel(DEFAULT_MODEL, (downloaded, total, file) => {
      if (file && file !== currentFile) {
        if (currentFile) {
          // Complete previous file's progress bar
          process.stdout.write('\n');
        }
        currentFile = file;
        fileCount++;
        console.log(`  ${dim(`[${fileCount}/4]`)} ${file}`);
      }

      if (total > 0) {
        const bar = progressBar(downloaded, total, 25);
        process.stdout.write(`\r  ${bar}`);
      }
    });

    console.log('\n');
    divider();
    console.log('');
    success('Model downloaded successfully');
    console.log('');
    info(`Run ${cyan('dybur start')} to begin`);
    console.log('');
  } catch (err) {
    console.log('\n');
    error(`Download failed: ${err}`);
    console.log('');
    info('Check your internet connection and try again');
    console.log('');
    process.exit(1);
  }
}

/**
 * Clean unused models
 */
async function cleanCommand(): Promise<void> {
  header('Clean Models');

  const spinner = new Spinner('Scanning for unused models');
  spinner.start();

  const removed = cleanModels();

  spinner.stop();

  if (removed.length === 0) {
    info('No unused models to remove');
    console.log('');
    return;
  }

  success(`Removed ${removed.length} model(s):`);
  console.log('');

  for (const name of removed) {
    console.log(`  ${dim(icons.bullet)} ${name}`);
  }

  console.log('');
}

export async function modelsCommand(args: string[]): Promise<void> {
  const subcommand = args[0];

  switch (subcommand) {
    case 'list':
      await listCommand();
      break;

    case 'prefetch':
    case 'download':
      await prefetchCommand();
      break;

    case 'clean':
      await cleanCommand();
      break;

    case undefined:
    case '--help':
    case '-h':
      showModelsHelp();
      break;

    default:
      error(`Unknown subcommand: ${subcommand}`);
      console.log('');
      showModelsHelp();
      process.exit(1);
  }
}
