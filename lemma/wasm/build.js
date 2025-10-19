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
const PROJECT_ROOT = join(__dirname, '..');
const WORKSPACE_ROOT = join(__dirname, '../..');

/**
 * Parse Cargo.toml metadata
 */
function parseCargoMetadata() {
  // Read workspace Cargo.toml
  const workspaceToml = readFileSync(join(WORKSPACE_ROOT, 'Cargo.toml'), 'utf8');

  // Read package Cargo.toml
  const packageToml = readFileSync(join(PROJECT_ROOT, 'Cargo.toml'), 'utf8');

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
                 'The programming language that means business.',
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
 * Build WASM package
 */
export function build() {
  console.log('Building WASM package...');

  // Run wasm-pack with web target (works in both browser and Node.js)
  try {
    execSync('wasm-pack build --target web --out-dir pkg', {
      stdio: 'inherit',
      cwd: PROJECT_ROOT
    });
  } catch (error) {
    console.error('Failed to build WASM:', error.message);
    process.exit(1);
  }

  // Parse metadata from Cargo.toml files
  const metadata = parseCargoMetadata();

  // Create package.json
  const packageJson = {
    name: "@benrogmans/lemma-engine",
    version: metadata.version,
    description: metadata.description,
    type: "module",
    main: "lemma.js",
    types: "lemma.d.ts",
    files: [
      "lemma_bg.wasm",
      "lemma.js",
      "lemma.d.ts",
      "lemma_bg.js",
      "lemma_bg.wasm.d.ts"
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

  // Write package.json to pkg directory
  const outputPath = join(PROJECT_ROOT, 'pkg', 'package.json');
  writeFileSync(outputPath, JSON.stringify(packageJson, null, 2) + '\n');

  // Copy README.md from wasm directory to pkg directory
  const readmeSource = join(PROJECT_ROOT, 'wasm', 'README.md');
  const readmeDest = join(PROJECT_ROOT, 'pkg', 'README.md');
  const readmeContent = readFileSync(readmeSource, 'utf8');
  writeFileSync(readmeDest, readmeContent);

  console.log('✓ WASM package built successfully');
  console.log(`✓ Created package.json for @benrogmans/lemma-engine@${metadata.version}`);
  console.log('✓ Copied README.md to pkg directory');
}

// CLI interface
if (import.meta.url === `file://${process.argv[1]}`) {
  build();
}
