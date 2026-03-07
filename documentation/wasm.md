---
title: WebAssembly
---

# Using Lemma Engine in the Browser

The Lemma Engine can be compiled to WebAssembly and used directly in web browsers.

## Installation

Install from NPM:

```bash
npm install @benrogmans/lemma-engine
```

## Usage

```javascript
import init, { WasmEngine } from '@benrogmans/lemma-engine';

await init();
const engine = new WasmEngine();

// Load a spec
const result = engine.addLemmaCode(`
  spec example
  fact price: 100
  rule total: price * 2
`, 'example.lemma');

const addResult = JSON.parse(result);
if (addResult.success) {
  console.log('Spec loaded successfully');
} else {
  console.error('Error:', addResult.error);
}

// Evaluate
const output = engine.evaluate('example', '[]');
const response = JSON.parse(output);

if (response.success) {
  const data = JSON.parse(response.data);
  console.log('Results:', data.results);
  if (response.warnings) {
    console.log('Warnings:', response.warnings);
  }
} else {
  console.error('Evaluation error:', response.error);
}
```

## API

### `new WasmEngine()`
Creates a new engine instance.

### `addLemmaCode(code: string, source: string): string`
Parses and loads a Lemma spec.

**Returns:** JSON string with `{success, data, error, warnings}` structure.

### `evaluate(specName: string, factValuesJson: string): string`
Evaluates a loaded spec.

**Parameters:**
- `specName` - Name of the spec to evaluate
- `factValuesJson` - JSON array of fact values (e.g., `'["x=10", "y=20"]'`)

**Returns:** JSON string with `{success, data, error, warnings}` structure. The `data` field contains the serialized `Response`.

### `listSpecs(): string`
Returns JSON string with `{success, data, error, warnings}` structure. The `data` field contains a JSON array of loaded spec names.


## Response Format

All methods return JSON strings with this structure:

```json
{
  "success": true,
  "data": "...",
  "error": null,
  "warnings": null
}
```

For `evaluate()`, the `data` field contains a serialized `Response`. The `warnings` field contains any warnings from evaluation.

## Building from Source

If you need to build the WASM package yourself:

```bash
cd lemma
node wasm/build.js
```

This generates JavaScript bindings in `engine/pkg/` with a package.json created from Cargo.toml metadata.

For comprehensive JavaScript API documentation and examples, see [engine/wasm/README.md](../engine/wasm/README.md).

