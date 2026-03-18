#!/usr/bin/env node
/**
 * WASM npm package: wasm-pack → rename glue → copy checked-in entrypoints.
 */

import {
  readFileSync,
  writeFileSync,
  copyFileSync,
  rmSync,
  existsSync,
  renameSync,
} from 'fs';
import { join, dirname, resolve } from 'path';
import { fileURLToPath, pathToFileURL } from 'url';
import { execSync } from 'child_process';

const __dirname = dirname(fileURLToPath(import.meta.url));
const PROJECT_ROOT = join(__dirname, '..');
const LSP_ROOT = join(PROJECT_ROOT, 'lsp');
const WORKSPACE_ROOT = join(__dirname, '../..');
const WASM_DIR = __dirname;
const PKG = join(PROJECT_ROOT, 'pkg');

/** wasm-pack --out-name lemma */
const PACK_JS = 'lemma.js';
const PACK_DTS = 'lemma.d.ts';
const BINDINGS_JS = 'lemma.bindings.js';
const BINDINGS_DTS = 'lemma.bindings.d.ts';
const WASM_FILE = 'lemma_bg.wasm';

/**
 * @benrogmans/lemma-engine — npm-only (not workspace.description / not lsp crate).
 */
const NPM_BRANDING = {
  description: 'A language that means business. Also in the browser.',
  homepage: 'https://github.com/benrogmans/lemma',
  keywords: [
    'lemma',
    'rules-engine',
    'business-rules',
    'policy-engine',
    'declarative',
    'dsl',
    'typed',
    'wasm',
    'webassembly',
    'lsp',
    'language-server',
    'monaco-editor',
  ],
};

function mustExist(dir, name, ctx) {
  const p = join(dir, name);
  if (!existsSync(p)) {
    throw new Error(`WASM build: missing ${name} (${ctx}). Path: ${p}`);
  }
}

/** Version, license, repo, author only — from workspace root Cargo.toml. */
function parseWorkspacePublishMeta() {
  const workspaceToml = readFileSync(join(WORKSPACE_ROOT, 'Cargo.toml'), 'utf8');
  const workspaceMatch = workspaceToml.match(
    /^\[workspace\.package\]\n((?:[^\[].*\n)*)/m
  );
  const workspaceSection = workspaceMatch ? workspaceMatch[1] : '';
  const extractField = (section, field) => {
    const m = section.match(new RegExp(`^${field} = "([^"]+)"`, 'm'));
    return m ? m[1] : null;
  };
  const authorsMatch = workspaceSection.match(/^authors = \[(.*?)\]/m);
  const author =
    authorsMatch?.[1].match(/"([^"]+)"/)?.[1] || 'Ben Rogmans';
  return {
    version: extractField(workspaceSection, 'version') || '0.0.0-dev',
    license: extractField(workspaceSection, 'license') || 'Apache-2.0',
    repository:
      extractField(workspaceSection, 'repository') ||
      'https://github.com/benrogmans/lemma',
    author,
  };
}

export function build() {
  console.log('Building WASM package (engine + LSP)…');

  for (const f of ['lemma-entry.js', 'lsp-entry.js', 'lemma.d.ts', 'lsp.d.ts']) {
    mustExist(WASM_DIR, f, 'checked-in sources');
  }

  const licenseSrc = join(WORKSPACE_ROOT, 'LICENSE');
  copyFileSync(licenseSrc, join(LSP_ROOT, 'LICENSE'));

  if (existsSync(PKG)) {
    rmSync(PKG, { recursive: true });
  }

  const env = {
    ...process.env,
    CARGO_PROFILE_RELEASE_OPT_LEVEL: 'z',
    CARGO_PROFILE_RELEASE_LTO: 'true',
    CARGO_PROFILE_RELEASE_STRIP: 'true',
    CARGO_PROFILE_RELEASE_CODEGEN_UNITS: '1',
  };
  try {
    execSync('wasm-pack build --target web --out-dir ../pkg --out-name lemma', {
      stdio: 'inherit',
      cwd: LSP_ROOT,
      env,
    });
  } catch {
    console.error('wasm-pack failed');
    process.exit(1);
  }

  mustExist(PKG, PACK_JS, 'after wasm-pack');
  mustExist(PKG, WASM_FILE, 'after wasm-pack');

  const packJsPath = join(PKG, PACK_JS);
  const bindingsJsPath = join(PKG, BINDINGS_JS);
  if (existsSync(bindingsJsPath)) {
    rmSync(bindingsJsPath);
  }
  renameSync(packJsPath, bindingsJsPath);

  const packDts = join(PKG, PACK_DTS);
  const bindingsDts = join(PKG, BINDINGS_DTS);
  if (existsSync(packDts)) {
    if (existsSync(bindingsDts)) {
      rmSync(bindingsDts);
    }
    renameSync(packDts, bindingsDts);
  }

  copyFileSync(join(WASM_DIR, 'lemma-entry.js'), join(PKG, 'lemma.js'));
  copyFileSync(join(WASM_DIR, 'lsp-entry.js'), join(PKG, 'lsp.js'));
  copyFileSync(join(WASM_DIR, 'lemma.d.ts'), join(PKG, 'lemma.d.ts'));
  copyFileSync(join(WASM_DIR, 'lsp.d.ts'), join(PKG, 'lsp.d.ts'));

  copyFileSync(join(WASM_DIR, 'lsp-client.js'), join(PKG, 'lsp-client.js'));
  copyFileSync(join(WASM_DIR, 'monaco.js'), join(PKG, 'monaco.js'));

  const meta = parseWorkspacePublishMeta();
  const packageJson = {
    name: '@benrogmans/lemma-engine',
    version: meta.version,
    description: NPM_BRANDING.description,
    type: 'module',
    main: 'lemma.js',
    types: 'lemma.d.ts',
    files: [
      WASM_FILE,
      BINDINGS_JS,
      BINDINGS_DTS,
      'lemma.js',
      'lemma.d.ts',
      'lsp.js',
      'lsp.d.ts',
      'lemma_bg.wasm.d.ts',
      'lsp-client.js',
      'monaco.js',
      'snippets',
    ],
    exports: {
      '.': './lemma.js',
      './lsp': './lsp.js',
      './lsp-client': './lsp-client.js',
      './monaco': './monaco.js',
    },
    keywords: [...NPM_BRANDING.keywords],
    author: meta.author,
    license: meta.license,
    repository: { type: 'git', url: meta.repository },
    homepage: NPM_BRANDING.homepage,
    bugs: { url: `${meta.repository}/issues` },
  };

  writeFileSync(
    join(PKG, 'package.json'),
    JSON.stringify(packageJson, null, 2) + '\n'
  );

  copyFileSync(join(WASM_DIR, 'README.md'), join(PKG, 'README.md'));

  mustExist(PKG, 'lemma.js', 'entry copy');
  mustExist(PKG, BINDINGS_JS, 'bindings rename');
  console.log('✓ WASM package built:', PKG);
}

const isMain =
  process.argv[1] &&
  import.meta.url === pathToFileURL(resolve(process.argv[1])).href;
if (isMain) {
  build();
}
