#!/usr/bin/env node
'use strict';

const { spawnSync } = require('child_process');

const PLATFORM_PACKAGES = {
  'linux-x64': '@lemmabase/cli-linux-x64',
  'linux-arm64': '@lemmabase/cli-linux-arm64',
  'darwin-x64': '@lemmabase/cli-darwin-x64',
  'darwin-arm64': '@lemmabase/cli-darwin-arm64',
  'win32-x64': '@lemmabase/cli-win32-x64',
  'win32-arm64': '@lemmabase/cli-win32-arm64',
};

const key = `${process.platform}-${process.arch}`;
const pkg = PLATFORM_PACKAGES[key];

if (!pkg) {
  console.error(
    `lemma: unsupported platform ${key}. ` +
      `Supported: ${Object.keys(PLATFORM_PACKAGES).join(', ')}. ` +
      `Install from source instead: cargo install lemma`
  );
  process.exit(1);
}

const exeName = process.platform === 'win32' ? 'lemma.exe' : 'lemma';

let binPath;
try {
  binPath = require.resolve(`${pkg}/bin/${exeName}`);
} catch (err) {
  console.error(
    `lemma: failed to locate ${pkg}/bin/${exeName}. ` +
      `The platform-specific package was not installed. ` +
      `Try reinstalling: npm install lemma`
  );
  console.error(err.message);
  process.exit(1);
}

const result = spawnSync(binPath, process.argv.slice(2), {
  stdio: 'inherit',
  windowsHide: false,
});

if (result.error) {
  console.error(`lemma: failed to spawn ${binPath}: ${result.error.message}`);
  process.exit(1);
}

if (result.signal) {
  process.kill(process.pid, result.signal);
} else {
  process.exit(result.status ?? 1);
}
