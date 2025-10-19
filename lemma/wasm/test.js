#!/usr/bin/env node

/**
 * Test script for Lemma WASM package
 */

import { readFileSync, existsSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const PROJECT_ROOT = join(__dirname, '..');

/**
 * Test WASM package
 */
export async function test() {
  console.log('Testing Lemma WASM...');

  // Suppress deprecation warnings
  process.removeAllListeners('warning');

  try {
    // Check if pkg directory exists
    const pkgPath = join(PROJECT_ROOT, 'pkg');
    if (!existsSync(join(pkgPath, 'lemma.js'))) {
      console.log('WASM not built. Run: node wasm/build.js');
      process.exit(1);
    }

    // Import the JS bindings
    const { WasmEngine, initSync } = await import('../pkg/lemma.js');

    // Load the WASM module
    const wasmPath = join(pkgPath, 'lemma_bg.wasm');
    const wasmBytes = readFileSync(wasmPath);

    // Initialize WASM synchronously
    initSync(wasmBytes);
    console.log('✓ WASM initialized successfully');

    // Test 1: Engine creation
    const engine = new WasmEngine();
    console.log('✓ Engine created successfully');

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
    console.log('✓ Document added successfully');

    // Test 3: Evaluate document
    const evalResult = engine.evaluate('test', '{}');
    const evalParsed = JSON.parse(evalResult);
    if (!evalParsed.success) {
      throw new Error('Failed to evaluate document: ' + JSON.stringify(evalParsed));
    }
    console.log('✓ Document evaluated successfully');

    // Test 4: List documents
    const listResult = engine.listDocuments();
    const listParsed = JSON.parse(listResult);
    if (!listParsed.success || listParsed.documents.length === 0) {
      throw new Error('Failed to list documents: ' + JSON.stringify(listParsed));
    }
    console.log('✓ Documents listed successfully');

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
    console.log('✓ Complex document added successfully');

    // Test 6: Evaluation with facts (as JSON object)
    const factsResult = engine.evaluate('pricing', JSON.stringify({
      quantity: 100,
      is_vip: true
    }));
    const factsParsed = JSON.parse(factsResult);
    if (!factsParsed.success) {
      throw new Error('Failed to evaluate with facts: ' + JSON.stringify(factsParsed));
    }
    console.log('✓ Evaluation with facts successful');

    // Test 7: Various fact value types
    const typesResult = engine.addLemmaCode(`
      doc type_test
      fact number_fact = 42
      fact bool_fact = false
      fact string_fact = "hello"
      fact unit_fact = 100 eur
      fact date_fact = 2024-01-15

      rule double_number = number_fact * 2
    `, 'type_test.lemma');

    const typesParsed = JSON.parse(typesResult);
    if (!typesParsed.success) {
      throw new Error('Failed to add type test document: ' + JSON.stringify(typesParsed));
    }

    // Test with various types in the object
    const typedFactsResult = engine.evaluate('type_test', JSON.stringify({
      number_fact: 50,
      bool_fact: true,
      string_fact: "world",
      unit_fact: "200 eur",
      date_fact: "2024-12-25"
    }));

    const typedFactsParsed = JSON.parse(typedFactsResult);
    if (!typedFactsParsed.success) {
      throw new Error('Failed to evaluate with typed facts: ' + JSON.stringify(typedFactsParsed));
    }

    // Verify the overrides worked by checking the rule result
    const doubleRule = typedFactsParsed.rules.double_number;
    if (!doubleRule) {
      throw new Error('double_number rule not found in results');
    }
    // The rule should have used the overridden value (50 * 2 = 100)
    if (!doubleRule.result || doubleRule.result.value !== "100") {
      throw new Error(`Expected double_number to be 100 (50*2), got ${doubleRule.result?.value}`);
    }
    if (doubleRule.result.type !== "number") {
      throw new Error(`Expected type to be nmber, got ${doubleRule.result.type}`);
    }
    console.log('✓ Type handling in facts object successful');

    console.log('\n✅ All WASM tests passed!');

  } catch (error) {
    console.error('\n❌ WASM test failed:', error.message);
    process.exit(1);
  }
}

// CLI interface
if (import.meta.url === `file://${process.argv[1]}`) {
  await test();
}
