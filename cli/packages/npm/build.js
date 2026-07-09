#!/usr/bin/env node
/**
 * lemma CLI npm package builder.
 *
 * Generates one umbrella package (`lemma`) plus one per-platform package
 * (`@lemmabase/cli-<os>-<arch>`) containing the prebuilt native binary.
 *
 * Usage:
 *   node build.js --binaries <dir> [--allow-partial]
 *
 * The <dir> must contain subdirectories named after each platform key
 * (e.g. linux-x64, darwin-arm64, win32-x64), each holding the corresponding
 * `lemma` (or `lemma.exe`) binary.
 */

import {
  readFileSync,
  writeFileSync,
  copyFileSync,
  mkdirSync,
  rmSync,
  existsSync,
  chmodSync,
} from 'fs';
import { join, dirname, resolve } from 'path';
import { fileURLToPath, pathToFileURL } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const PROJECT_ROOT = join(__dirname, '..', '..', '..');
const NPM_DIR = join(PROJECT_ROOT, 'cli', 'packages', 'npm');
const DIST = join(NPM_DIR, 'dist');
const UMBRELLA_DIST = join(DIST, 'lemma');
const PLATFORMS_DIST = join(DIST, 'platforms');

const REPOSITORY = 'https://github.com/lemma/lemma';
const LICENSE = 'Apache-2.0';
const AUTHOR = 'Ben Rogmans <ben@lemmabase.com>';
const UMBRELLA_DESCRIPTION = 'A pure, declarative language for business rules.';
const UMBRELLA_KEYWORDS = [
  'lemma',
  'cli',
  'rules-engine',
  'business-rules',
  'policy-engine',
  'declarative',
  'dsl',
  'typed',
];

/**
 * Platform targets. Keys are `${process.platform}-${process.arch}`,
 * matching the launcher map in bin/lemma.js.
 */
const PLATFORMS = [
  { key: 'linux-x64', os: 'linux', cpu: 'x64', exe: 'lemma' },
  { key: 'linux-arm64', os: 'linux', cpu: 'arm64', exe: 'lemma' },
  { key: 'darwin-x64', os: 'darwin', cpu: 'x64', exe: 'lemma' },
  { key: 'darwin-arm64', os: 'darwin', cpu: 'arm64', exe: 'lemma' },
  { key: 'win32-x64', os: 'win32', cpu: 'x64', exe: 'lemma.exe' },
  { key: 'win32-arm64', os: 'win32', cpu: 'arm64', exe: 'lemma.exe' },
];

function parseArgs(argv) {
  const args = { binaries: null, allowPartial: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--binaries') {
      args.binaries = argv[++i];
    } else if (a === '--allow-partial') {
      args.allowPartial = true;
    } else if (a === '--help' || a === '-h') {
      console.log(
        'Usage: node build.js --binaries <dir> [--allow-partial]\n\n' +
          'The <dir> must contain subdirectories named after each platform key\n' +
          '(linux-x64, linux-arm64, darwin-x64, darwin-arm64, win32-x64, win32-arm64),\n' +
          'each holding the corresponding `lemma` (or `lemma.exe`) binary.'
      );
      process.exit(0);
    } else {
      throw new Error(`Unknown argument: ${a}`);
    }
  }
  if (!args.binaries) {
    throw new Error('Missing required --binaries <dir>');
  }
  return args;
}

/**
 * Reads version, license, author, repository from the workspace Cargo.toml.
 * Mirrors the parser in engine/packages/npm/build.js so the two scripts stay
 * in sync on metadata extraction.
 */
function parseWorkspaceMeta() {
  const toml = readFileSync(join(PROJECT_ROOT, 'Cargo.toml'), 'utf8');
  const match = toml.match(/^\[workspace\.package\]\n((?:[^\[].*\n)*)/m);
  const section = match ? match[1] : '';
  const field = (name) => {
    const m = section.match(new RegExp(`^${name} = "([^"]+)"`, 'm'));
    return m ? m[1] : null;
  };
  const authorsMatch = section.match(/^authors = \[(.*?)\]/m);
  const author = authorsMatch?.[1].match(/"([^"]+)"/)?.[1] || AUTHOR;
  const version = field('version');
  if (!version) {
    throw new Error('Could not read workspace.package.version from Cargo.toml');
  }
  return {
    version,
    license: field('license') || LICENSE,
    repository: field('repository') || REPOSITORY,
    author,
  };
}

function ensureCleanDist() {
  if (existsSync(DIST)) {
    rmSync(DIST, { recursive: true });
  }
  mkdirSync(DIST, { recursive: true });
  mkdirSync(UMBRELLA_DIST, { recursive: true });
  mkdirSync(PLATFORMS_DIST, { recursive: true });
}

function writeJson(path, value) {
  writeFileSync(path, JSON.stringify(value, null, 2) + '\n');
}

function buildPlatformPackage(platform, binariesDir, meta) {
  const src = join(binariesDir, platform.key, platform.exe);
  if (!existsSync(src)) {
    return { built: false, src };
  }

  const pkgDir = join(PLATFORMS_DIST, platform.key);
  const binDir = join(pkgDir, 'bin');
  mkdirSync(binDir, { recursive: true });

  const dest = join(binDir, platform.exe);
  copyFileSync(src, dest);
  chmodSync(dest, 0o755);

  copyFileSync(join(PROJECT_ROOT, 'LICENSE'), join(pkgDir, 'LICENSE'));

  const pkgJson = {
    name: `@lemmabase/cli-${platform.key}`,
    version: meta.version,
    description: `${UMBRELLA_DESCRIPTION} Prebuilt CLI binary for ${platform.os}-${platform.cpu}.`,
    os: [platform.os],
    cpu: [platform.cpu],
    files: ['bin/', 'LICENSE'],
    author: meta.author,
    license: meta.license,
    repository: { type: 'git', url: `git+${meta.repository}.git` },
    homepage: meta.repository,
    bugs: { url: `${meta.repository}/issues` },
  };
  writeJson(join(pkgDir, 'package.json'), pkgJson);

  return { built: true, dir: pkgDir };
}

function buildUmbrellaPackage(meta) {
  const binDir = join(UMBRELLA_DIST, 'bin');
  mkdirSync(binDir, { recursive: true });

  copyFileSync(join(NPM_DIR, 'bin', 'lemma.js'), join(binDir, 'lemma.js'));
  chmodSync(join(binDir, 'lemma.js'), 0o755);

  copyFileSync(join(NPM_DIR, 'README.md'), join(UMBRELLA_DIST, 'README.md'));
  copyFileSync(join(PROJECT_ROOT, 'LICENSE'), join(UMBRELLA_DIST, 'LICENSE'));

  const optionalDeps = Object.fromEntries(
    PLATFORMS.map((p) => [`@lemmabase/cli-${p.key}`, meta.version])
  );

  const pkgJson = {
    name: 'lemma',
    version: meta.version,
    description: UMBRELLA_DESCRIPTION,
    bin: { lemma: 'bin/lemma.js' },
    files: ['bin/', 'README.md', 'LICENSE'],
    optionalDependencies: optionalDeps,
    keywords: UMBRELLA_KEYWORDS,
    author: meta.author,
    license: meta.license,
    repository: { type: 'git', url: `git+${meta.repository}.git` },
    homepage: meta.repository,
    bugs: { url: `${meta.repository}/issues` },
    engines: { node: '>=18' },
  };
  writeJson(join(UMBRELLA_DIST, 'package.json'), pkgJson);
}

export function build({ binaries, allowPartial }) {
  const meta = parseWorkspaceMeta();
  console.log(`Building lemma npm packages at version ${meta.version}`);

  if (!existsSync(binaries)) {
    throw new Error(`Binaries directory not found: ${binaries}`);
  }

  ensureCleanDist();

  const missing = [];
  const built = [];
  for (const platform of PLATFORMS) {
    const result = buildPlatformPackage(platform, binaries, meta);
    if (result.built) {
      built.push(platform.key);
      console.log(`  built @lemmabase/cli-${platform.key}`);
    } else {
      missing.push({ key: platform.key, src: result.src });
    }
  }

  if (missing.length > 0) {
    const msg = missing
      .map((m) => `  - ${m.key} (expected ${m.src})`)
      .join('\n');
    if (!allowPartial) {
      throw new Error(
        `Missing binaries for ${missing.length}/${PLATFORMS.length} platforms:\n${msg}\n` +
          `Pass --allow-partial to build a subset (for local testing only).`
      );
    }
    console.warn(`WARNING: skipping ${missing.length} platform(s):\n${msg}`);
  }

  buildUmbrellaPackage(meta);
  console.log(`  built lemma (umbrella)`);

  console.log(`\nDone. Output: ${DIST}`);
  console.log(`Platform packages: ${built.join(', ')}`);
}

const isMain =
  process.argv[1] &&
  import.meta.url === pathToFileURL(resolve(process.argv[1])).href;

if (isMain) {
  try {
    const args = parseArgs(process.argv.slice(2));
    build(args);
  } catch (err) {
    console.error(`build.js: ${err.message}`);
    process.exit(1);
  }
}
