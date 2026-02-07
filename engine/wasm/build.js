#!/usr/bin/env node

/**
 * Build script for Lemma WASM package
 */

import { readFileSync, writeFileSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
import { execSync } from 'child_process';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const PROJECT_ROOT = join(__dirname, '..');       // engine/
const LSP_ROOT = join(PROJECT_ROOT, 'lsp');       // engine/lsp/
const WORKSPACE_ROOT = join(__dirname, '../..');

/**
 * Parse Cargo.toml metadata
 */
function parseCargoMetadata() {
  // Read workspace Cargo.toml
  const workspaceToml = readFileSync(join(WORKSPACE_ROOT, 'Cargo.toml'), 'utf8');

  // Read LSP package Cargo.toml (the one we build for WASM)
  const packageToml = readFileSync(join(LSP_ROOT, 'Cargo.toml'), 'utf8');

  // Extract workspace.package section
  const workspaceMatch = workspaceToml.match(/^\[workspace\.package\]\n((?:[^\[].*\n)*)/m);
  const workspaceSection = workspaceMatch ? workspaceMatch[1] : '';

  // Extract package section
  const packageMatch = packageToml.match(/^\[package\]\n((?:[^\[].*\n)*)/m);
  const packageSection = packageMatch ? packageMatch[1] : '';

  // Helper to extract field value
  const extractField = (section, field) => {
    const match = section.match(new RegExp(`^${field} = "([^"]+)"`, 'm'));
    return match ? match[1] : null;
  };

  // Extract metadata (package overrides workspace)
  const metadata = {
    version: extractField(workspaceSection, 'version') || '0.0.0-dev',
    license: extractField(workspaceSection, 'license') || 'Apache-2.0',
    repository: extractField(workspaceSection, 'repository') || 'https://github.com/benrogmans/lemma',
    description: extractField(packageSection, 'description') ||
                 extractField(workspaceSection, 'description') ||
                 'A language that means business.',
    homepage: extractField(packageSection, 'homepage') || 'https://github.com/benrogmans/lemma',
    keywords: []
  };

  // Extract authors (it's an array in TOML)
  const authorsMatch = workspaceSection.match(/^authors = \[(.*?)\]/m);
  if (authorsMatch) {
    const authorString = authorsMatch[1].match(/"([^"]+)"/)?.[1];
    metadata.author = authorString || 'Ben Rogmans';
  } else {
    metadata.author = 'Ben Rogmans';
  }

  // Extract keywords array
  const keywordsMatch = packageSection.match(/^keywords = \[(.*?)\]/m);
  if (keywordsMatch) {
    metadata.keywords = keywordsMatch[1]
      .split(',')
      .map(k => k.trim().replace(/"/g, ''))
      .filter(k => k);
  }

  return metadata;
}

/**
 * Build WASM package (engine + LSP in one artifact from the lsp crate).
 */
export function build() {
  console.log('Building WASM package (engine + LSP)...');

  // Build the lsp crate for wasm32; it includes the engine and re-exports WasmEngine (playground uses only that).
  // Output to engine/pkg so the playground can load lemma.js from ../pkg/
  const env = {
    ...process.env,
    CARGO_PROFILE_RELEASE_OPT_LEVEL: 'z',
    CARGO_PROFILE_RELEASE_LTO: 'true',
    CARGO_PROFILE_RELEASE_STRIP: 'true',
    CARGO_PROFILE_RELEASE_CODEGEN_UNITS: '1'
  };
  try {
    execSync('wasm-pack build --target web --out-dir ../pkg', {
      stdio: 'inherit',
      cwd: LSP_ROOT,
      env
    });
  } catch (error) {
    console.error('Failed to build WASM:', error.message);
    process.exit(1);
  }

  // Parse metadata from Cargo.toml files
  const metadata = parseCargoMetadata();

  // Create package.json (lsp crate outputs lsp.js, lsp_bg.wasm)
  const packageJson = {
    name: "@benrogmans/lemma-engine",
    version: metadata.version,
    description: metadata.description,
    type: "module",
    main: "lsp.js",
    types: "lsp.d.ts",
    files: [
      "lsp_bg.wasm",
      "lsp.js",
      "lsp.d.ts",
      "lsp_bg.js",
      "lsp_bg.wasm.d.ts"
    ],
    keywords: [...metadata.keywords, "wasm", "webassembly"],
    author: metadata.author,
    license: metadata.license,
    repository: {
      type: "git",
      url: metadata.repository
    },
    homepage: metadata.homepage,
    bugs: {
      url: `${metadata.repository}/issues`
    }
  };

  // Write package.json to engine/pkg directory
  const outputPath = join(PROJECT_ROOT, 'pkg', 'package.json');
  writeFileSync(outputPath, JSON.stringify(packageJson, null, 2) + '\n');

  // Copy README.md from wasm directory to pkg directory
  const readmeSource = join(PROJECT_ROOT, 'wasm', 'README.md');
  const readmeDest = join(PROJECT_ROOT, 'pkg', 'README.md');
  const readmeContent = readFileSync(readmeSource, 'utf8');
  writeFileSync(readmeDest, readmeContent);

  console.log('✓ WASM package built successfully');
}

// CLI interface
if (import.meta.url === `file://${process.argv[1]}`) {
  build();
}
