import { defineConfig } from 'tsup';

export default defineConfig({
  entry: ['src/cli.ts'],
  format: ['esm'],
  target: 'node18',
  outDir: 'dist',
  clean: true,
  dts: false,
  sourcemap: false,
  minify: false,
  // Bundle workspace dependencies into the output
  noExternal: ['@dybur/core', '@dybur/config'],
  // Add shebang for CLI execution
  banner: {
    js: '#!/usr/bin/env node',
  },
});
