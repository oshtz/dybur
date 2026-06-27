/**
 * Models command - manage speech recognition models
 */

import {
  listModels,
  downloadModel,
  cleanModels,
  DEFAULT_MODEL,
  isModelInstalled,
  getAvailableModels,
  getModelDefinition,
  getDefaultModelDefinition,
  formatBytes,
} from '@dybur/core';
import { getModelsDir, loadConfig, updateConfig } from '@dybur/config';
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
  select,
} from '../ui.js';

function showModelsHelp(): void {
  header('Model Management');

  console.log(
    `  ${dim('dybur supports multiple STT models with different accuracy/speed tradeoffs.')}`
  );
  console.log(`  ${dim('Models are downloaded from HuggingFace.')}`);
  console.log('');

  divider();
  console.log('');

  console.log(`  ${brand.accent('Commands')}`);
  command('m, m l, m list', 'List installed models');
  command('m list -a', 'Show all available models');
  command('m d, m download', 'Download a model (interactive)');
  command('m s, m set', 'Set active model (interactive)');
  command('m prefetch', 'Download default model');
  command('m clean', 'Remove unused models');
  console.log('');

  console.log(`  ${brand.accent('Examples')}`);
  console.log(`  ${cyan('dybur m d')}                    ${dim('Interactive model download')}`);
  console.log(`  ${cyan('dybur m s')}                    ${dim('Interactive model selection')}`);
  console.log(
    `  ${cyan('dybur m d whisper-large-v3-turbo-int8')}  ${dim('Download specific model')}`
  );
  console.log('');

  const defaultModel = getDefaultModelDefinition();
  console.log(`  ${brand.accent('Default Model')}`);
  console.log(`  ${dim('ID:')}     ${defaultModel.id}`);
  console.log(`  ${dim('Name:')}   ${defaultModel.displayName}`);
  console.log(`  ${dim('Size:')}   ${formatBytes(defaultModel.sizeBytes)}`);
  console.log('');

  console.log(`  ${brand.accent('Available Models')}`);
  const models = getAvailableModels();
  for (const m of models) {
    const badge = m.isDefault ? ` ${green('[default]')}` : '';
    console.log(`  ${dim(icons.bullet)} ${m.id}${badge} - ${formatBytes(m.sizeBytes)}`);
  }
  console.log('');
}

/**
 * List installed models
 */
async function listCommand(showAvailable: boolean = false): Promise<void> {
  const modelsDir = getModelsDir();
  const config = loadConfig();
  const activeModelId = config.model ?? DEFAULT_MODEL;

  if (showAvailable) {
    // Show all available models from registry
    header('Available Models');

    const availableModels = getAvailableModels();

    for (const model of availableModels) {
      const installed = isModelInstalled(model.id);
      const isActive = model.id === activeModelId;
      const badges: string[] = [];

      if (model.isDefault) badges.push(green('[default]'));
      if (installed) badges.push(green('[installed]'));
      if (isActive && installed) badges.push(cyan('[active]'));

      const badgeStr = badges.length > 0 ? ` ${badges.join(' ')}` : '';
      const size = formatBytes(model.sizeBytes);

      console.log(`  ${brand.accent(icons.bullet)} ${model.id}${badgeStr}`);
      console.log(`    ${dim('Name:')} ${model.displayName}`);
      console.log(`    ${dim('Description:')} ${model.description}`);
      console.log(`    ${dim('Size:')} ${size}`);
      console.log(`    ${dim('Architecture:')} ${model.architecture}`);
      if (model.languages.length > 0) {
        console.log(`    ${dim('Languages:')} ${model.languages.join(', ')}`);
      } else {
        console.log(`    ${dim('Languages:')} All (99+)`);
      }
      console.log('');
    }

    divider();
    console.log('');
    console.log(`  ${dim('To download a model:')} ${cyan('dybur models download <model-id>')}`);
    console.log(`  ${dim('To set active model:')} ${cyan('dybur models set <model-id>')}`);
    console.log('');
    return;
  }

  // Show installed models
  header('Installed Models');

  const models = listModels();

  if (models.length === 0) {
    info('No models installed');
    console.log('');
    console.log(`  ${dim('To install the default model:')}`);
    console.log(`  ${cyan(`dybur models download ${DEFAULT_MODEL}`)}`);
    console.log('');
    console.log(`  ${dim('To see all available models:')}`);
    console.log(`  ${cyan('dybur models list --available')}`);
    console.log('');
    console.log(`  ${dim('Models directory:')} ${formatPath(modelsDir, 45)}`);
    console.log('');
    return;
  }

  for (const model of models) {
    const isActive = model.name === activeModelId;
    const badges: string[] = [];

    if (model.isDefault) badges.push(green('[default]'));
    if (isActive) badges.push(cyan('[active]'));

    const badgeStr = badges.length > 0 ? ` ${badges.join(' ')}` : '';
    const size = formatSize(model.size);

    // Get model definition for extra info
    const modelDef = getModelDefinition(model.name);

    console.log(`  ${brand.accent(icons.bullet)} ${model.name}${badgeStr}`);
    if (modelDef) {
      console.log(`    ${dim('Name:')} ${modelDef.displayName}`);
    }
    console.log(`    ${dim('Size:')} ${size}`);

    if (model.metadata) {
      console.log(`    ${dim('Downloaded:')} ${model.metadata.downloadedAt.split('T')[0]}`);
      if (model.metadata.source) {
        console.log(`    ${dim('Source:')} ${model.metadata.source}`);
      }
    }

    console.log('');
  }

  divider();
  console.log('');
  console.log(`  ${dim('Active model:')} ${activeModelId}`);
  console.log(`  ${dim('Models directory:')} ${formatPath(modelsDir, 45)}`);
  console.log('');
}

/**
 * Interactive model selection for download
 */
async function selectModelForDownload(): Promise<string | undefined> {
  const availableModels = getAvailableModels();
  const config = loadConfig();
  const activeModelId = config.model ?? DEFAULT_MODEL;

  // Build choices - show not-installed models first, then installed
  const notInstalled = availableModels.filter((m) => !isModelInstalled(m.id));
  const installed = availableModels.filter((m) => isModelInstalled(m.id));

  if (notInstalled.length === 0) {
    info('All models are already installed');
    console.log('');
    return undefined;
  }

  const choices = [
    ...notInstalled.map((model) => ({
      label: `${model.displayName} (${formatBytes(model.sizeBytes)})`,
      value: model.id,
      hint: model.description,
    })),
    // Add separator if there are installed models
    ...(installed.length > 0
      ? [
          {
            label: dim('--- Already Installed ---'),
            value: '__separator__',
            hint: '',
          },
          ...installed.map((model) => ({
            label: `${model.displayName} (${formatBytes(model.sizeBytes)})`,
            value: model.id,
            hint:
              model.id === activeModelId
                ? `${green('[installed]')} ${cyan('[active]')}`
                : green('[installed]'),
          })),
        ]
      : []),
  ];

  const selected = await select({
    message: 'Select model to download',
    choices,
    initial: 0,
  });

  // User cancelled or selected separator
  if (selected === null || selected === '__separator__') {
    return undefined;
  }

  return selected;
}

/**
 * Download a specific model (or show selector if no ID provided)
 */
async function downloadCommand(modelId?: string): Promise<void> {
  header('Download Model');

  // If no model ID provided, show interactive selector
  if (!modelId) {
    const selectedId = await selectModelForDownload();
    if (!selectedId) {
      info('Download cancelled');
      console.log('');
      return;
    }
    modelId = selectedId;
  }

  // Get model definition
  const modelDef = getModelDefinition(modelId);
  if (!modelDef) {
    error(`Unknown model: ${modelId}`);
    console.log('');
    console.log(`  ${dim('Available models:')}`);
    for (const m of getAvailableModels()) {
      console.log(`  ${dim(icons.bullet)} ${m.id}`);
    }
    console.log('');
    process.exit(1);
  }

  if (isModelInstalled(modelId)) {
    success(`Model already installed: ${modelId}`);
    console.log('');
    console.log(`  ${dim('To set as active:')} ${cyan(`dybur models set ${modelId}`)}`);
    console.log('');
    return;
  }

  console.log(`  ${dim('Model:')}  ${modelDef.displayName}`);
  console.log(`  ${dim('ID:')}     ${modelDef.id}`);
  console.log(`  ${dim('Size:')}   ${formatBytes(modelDef.sizeBytes)}`);
  console.log(`  ${dim('Source:')} huggingface.co/${modelDef.repo}`);
  console.log('');

  divider();
  console.log('');

  let currentFile = '';

  try {
    await downloadModel(modelId, (downloaded, total, file) => {
      if (file && file !== currentFile) {
        if (currentFile) {
          process.stdout.write('\n');
        }
        currentFile = file;
        console.log(`  ${file}`);
      }

      if (total > 0) {
        const bar = progressBar(downloaded, total, 25);
        process.stdout.write(`\r  ${bar}`);
      }
    });

    console.log('\n');
    divider();
    console.log('');
    success(`Model downloaded successfully: ${modelId}`);
    console.log('');
    console.log(`  ${dim('To set as active:')} ${cyan(`dybur models set ${modelId}`)}`);
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
 * Download the default model (alias for download DEFAULT_MODEL)
 */
async function prefetchCommand(): Promise<void> {
  await downloadCommand(DEFAULT_MODEL);
}

/**
 * Interactive model selection for setting active model
 */
async function selectModelForSet(): Promise<string | undefined> {
  const installedModels = listModels();
  const config = loadConfig();
  const activeModelId = config.model ?? DEFAULT_MODEL;

  if (installedModels.length === 0) {
    error('No models installed');
    console.log('');
    console.log(`  ${dim('To download a model:')}`);
    console.log(`  ${cyan('dybur models download')}`);
    console.log('');
    return undefined;
  }

  const choices = installedModels.map((model) => {
    const modelDef = getModelDefinition(model.name);
    const isActive = model.name === activeModelId;
    return {
      label: modelDef?.displayName ?? model.name,
      value: model.name,
      hint: isActive ? cyan('[active]') : (modelDef?.description ?? ''),
    };
  });

  // Find current selection index
  const currentIndex = choices.findIndex((c) => c.value === activeModelId);

  const selected = await select({
    message: 'Select active model',
    choices,
    initial: currentIndex >= 0 ? currentIndex : 0,
  });

  return selected ?? undefined;
}

/**
 * Set the active model (or show selector if no ID provided)
 */
async function setCommand(modelId?: string): Promise<void> {
  header('Set Active Model');

  // If no model ID provided, show interactive selector
  if (!modelId) {
    const selectedId = await selectModelForSet();
    if (!selectedId) {
      info('Selection cancelled');
      console.log('');
      return;
    }
    modelId = selectedId;
  }

  // Get model definition
  const modelDef = getModelDefinition(modelId);
  if (!modelDef) {
    error(`Unknown model: ${modelId}`);
    console.log('');
    console.log(`  ${dim('Available models:')}`);
    for (const m of getAvailableModels()) {
      console.log(`  ${dim(icons.bullet)} ${m.id}`);
    }
    console.log('');
    process.exit(1);
  }

  // Check if installed
  if (!isModelInstalled(modelId)) {
    error(`Model not installed: ${modelId}`);
    console.log('');
    console.log(`  ${dim('To download this model:')}`);
    console.log(`  ${cyan(`dybur models download ${modelId}`)}`);
    console.log('');
    process.exit(1);
  }

  // Update config
  const config = loadConfig();
  const oldModelId = config.model ?? DEFAULT_MODEL;

  if (oldModelId === modelId) {
    info(`Model already active: ${modelId}`);
    console.log('');
    return;
  }

  updateConfig({ model: modelId });

  success(`Active model changed: ${oldModelId} -> ${modelId}`);
  console.log('');
  console.log(`  ${dim('Name:')} ${modelDef.displayName}`);
  console.log(`  ${dim('Architecture:')} ${modelDef.architecture}`);
  console.log('');
  info(`Restart dybur for changes to take effect`);
  console.log('');
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
    case 'l': {
      // Check for --available flag
      const showAvailable = args.includes('--available') || args.includes('-a');
      await listCommand(showAvailable);
      break;
    }

    case 'download':
    case 'd': {
      // Download a model - interactive if no ID provided
      const downloadModelId = args[1];
      await downloadCommand(downloadModelId);
      break;
    }

    case 'set':
    case 's': {
      // Set active model - interactive if no ID provided
      const setModelId = args[1];
      await setCommand(setModelId);
      break;
    }

    case 'prefetch':
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
