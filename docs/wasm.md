---
title: WebAssembly
---

# Using Lemma Engine in the Browser

The Lemma Engine can be compiled to WebAssembly and used directly in web browsers.

## Installation

Install from NPM:

```bash
npm install @lemma/engine
```

## Usage

```javascript
import init, { WasmEngine } from '@lemma/engine';

await init();
const engine = new WasmEngine();

// Load a document
const result = engine.addLemmaCode(`
  doc example
  fact price = 100
  rule total = price * 2
`, 'example.lemma');

const addResult = JSON.parse(result);
if (addResult.success) {
  console.log('Document loaded successfully');
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
Parses and loads a Lemma document.

**Returns:** JSON string with `{success, data, error, warnings}` structure.

### `evaluate(docName: string, factValuesJson: string): string`
Evaluates a loaded document.

**Parameters:**
- `docName` - Name of the document to evaluate
- `factValuesJson` - JSON array of fact overrides (e.g., `'["x=10", "y=20"]'`)

**Returns:** JSON string with `{success, data, error, warnings}` structure. The `data` field contains the serialized `Response`.

### `listDocuments(): string`
Returns JSON string with `{success, data, error, warnings}` structure. The `data` field contains a JSON array of loaded document names.


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
cargo install wasm-pack
wasm-pack build lemma --target web --out-dir pkg
```

This generates JavaScript bindings in `lemma/pkg/`.

