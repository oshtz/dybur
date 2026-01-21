/**
 * Stop command - stops the background service
 */

import { exec } from 'child_process';
import { promisify } from 'util';
import { isMacOS, isWindows } from '@dybur/config';
import { header, info, Spinner } from '../ui.js';

const execAsync = promisify(exec);

/**
 * Find and kill the dybur tray process
 */
async function killTrayProcess(): Promise<boolean> {
  try {
    if (isWindows()) {
      await execAsync('taskkill /IM dybur.exe /F');
      return true;
    } else if (isMacOS()) {
      await execAsync('pkill -f dybur');
      return true;
    }
  } catch {
    return false;
  }

  return false;
}

export async function stopCommand(_args: string[]): Promise<void> {
  header('Stopping dybur');

  const spinner = new Spinner('Stopping service');
  spinner.start();

  const killed = await killTrayProcess();

  if (killed) {
    spinner.succeed('dybur stopped');
  } else {
    spinner.stop();
    info('dybur was not running');
  }

  console.log('');
}
