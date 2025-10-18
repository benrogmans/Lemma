#!/usr/bin/env node

/**
 * Simple WASM test script
 * Run with: npm test
 */


import { readFileSync } from 'fs';
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

async function testWasm() {
    console.log('Testing Lemma WASM...');

    try {
        // Check if pkg directory exists
        const pkgPath = join(__dirname, 'pkg');
        try {
            readFileSync(join(pkgPath, 'lemma.js'));
        } catch {
            console.log('WASM not built. Run: wasm-pack build --target web --out-dir pkg && cp package.json pkg/package.json');
            process.exit(1);
        }

        // Import the JS bindings
        const { WasmEngine, initSync } = await import('./pkg/lemma.js');

        // Load the WASM module
        const wasmPath = join(pkgPath, 'lemma_bg.wasm');
        const wasmBytes = readFileSync(wasmPath);

        // Initialize WASM synchronously
        initSync(wasmBytes);

        console.log('WASM initialized successfully');

        // Test 1: Engine creation
        const engine = new WasmEngine();
        console.log('Engine created successfully');

        // Test 2: Add simple document
        const addResult = engine.addLemmaCode(`
            doc test
            fact x = 10
            rule double = x * 2
        `, 'test.lemma');

        const addParsed = JSON.parse(addResult);
        if (!addParsed.success) {
            throw new Error('Failed to add document: ' + JSON.stringify(addParsed));
        }
        console.log('Document added successfully');

        // Test 3: Evaluate document
        const evalResult = engine.evaluate('test', '[]');
        const evalParsed = JSON.parse(evalResult);
        if (!evalParsed.success) {
            throw new Error('Failed to evaluate document: ' + JSON.stringify(evalParsed));
        }
        console.log('Document evaluated successfully');

        // Test 4: List documents
        const listResult = engine.listDocuments();
        const listParsed = JSON.parse(listResult);
        if (!listParsed.success || listParsed.data.length === 0) {
            throw new Error('Failed to list documents: ' + JSON.stringify(listParsed));
        }
        console.log('Documents listed successfully');

        // Test 5: Complex document
        const complexResult = engine.addLemmaCode(`
            doc pricing
            fact quantity = 25
            fact is_vip = false

            rule discount = 0%
              unless quantity >= 10 then 10%
              unless quantity >= 50 then 20%
              unless is_vip then 25%

            rule price = 200 eur - discount?
        `, 'pricing.lemma');

        const complexParsed = JSON.parse(complexResult);
        if (!complexParsed.success) {
            throw new Error('Failed to add complex document: ' + JSON.stringify(complexParsed));
        }
        console.log('Complex document added successfully');

        console.log('\nAll WASM tests passed!');

    } catch (error) {
        console.error('WASM test failed:', error.message);
        process.exit(1);
    }
}

testWasm();
