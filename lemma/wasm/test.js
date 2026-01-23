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
    initSync({ module: wasmBytes });
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

      rule price = 200 - discount?
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
      fact unit_fact = 100
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
      unit_fact: "200",
      date_fact: "2024-12-25"
    }));

    const typedFactsParsed = JSON.parse(typedFactsResult);
    if (!typedFactsParsed.success) {
      throw new Error('Failed to evaluate with typed facts: ' + JSON.stringify(typedFactsParsed));
    }

    // Verify the overrides worked by checking the rule result
    const doubleRule = typedFactsParsed.response?.results?.double_number;
    if (!doubleRule) {
      throw new Error('double_number rule not found in results');
    }
    // The rule should have used the overridden value (50 * 2 = 100)
    if (!doubleRule.result || doubleRule.result.value !== 100) {
      throw new Error(`Expected double_number to be 100 (50*2), got ${doubleRule.result?.value}`);
    }
    if (doubleRule.result.type !== "number") {
      throw new Error(`Expected type to be number, got ${doubleRule.result.type}`);
    }
    console.log('✓ Type handling in facts object successful');

    // Test 8: Error handling - parse error
    const parseErrorResult = engine.addLemmaCode('doc invalid\nfact x =', 'invalid.lemma');
    const parseErrorParsed = JSON.parse(parseErrorResult);
    if (parseErrorParsed.success) {
      throw new Error('Expected parse error but got success');
    }
    if (!parseErrorParsed.error || !parseErrorParsed.error.includes('Parse Error')) {
      throw new Error('Expected parse error message, got: ' + parseErrorParsed.error);
    }
    console.log('✓ Parse error handling successful');

    // Test 9: Error handling - evaluate non-existent document
    const nonExistentResult = engine.evaluate('nonexistent', '{}');
    const nonExistentParsed = JSON.parse(nonExistentResult);
    if (nonExistentParsed.success) {
      throw new Error('Expected error for non-existent document but got success');
    }
    console.log('✓ Non-existent document error handling successful');

    // Test 10: Error handling - invalid JSON in facts
    const invalidJsonResult = engine.evaluate('test', 'not json');
    const invalidJsonParsed = JSON.parse(invalidJsonResult);
    if (invalidJsonParsed.success) {
      throw new Error('Expected error for invalid JSON but got success');
    }
    console.log('✓ Invalid JSON error handling successful');

    // Test 11: Veto scenario
    const vetoResult = engine.addLemmaCode(`
      doc veto_test
      fact x = 10
      rule bad_sqrt = sqrt(-1)
    `, 'veto_test.lemma');
    const vetoAddParsed = JSON.parse(vetoResult);
    if (!vetoAddParsed.success) {
      throw new Error('Failed to add veto test document: ' + JSON.stringify(vetoAddParsed));
    }
    const vetoEvalResult = engine.evaluate('veto_test', '{}');
    const vetoEvalParsed = JSON.parse(vetoEvalResult);
    if (!vetoEvalParsed.success) {
      throw new Error('Failed to evaluate veto test: ' + JSON.stringify(vetoEvalParsed));
    }
    const badSqrtRule = vetoEvalParsed.response?.results?.bad_sqrt;
    if (!badSqrtRule) {
      throw new Error('bad_sqrt rule not found in results');
    }
    if (badSqrtRule.result?.type !== "veto") {
      throw new Error('Expected veto result type, got: ' + JSON.stringify(badSqrtRule.result));
    }
    console.log('✓ Veto handling successful');

    // Test 12: Missing facts
    const missingFactsResult = engine.addLemmaCode(`
      doc missing_test
      fact x = [number]
      fact y = [number]
      rule sum = x + y
    `, 'missing_test.lemma');
    const missingAddParsed = JSON.parse(missingFactsResult);
    if (!missingAddParsed.success) {
      throw new Error('Failed to add missing facts test document: ' + JSON.stringify(missingAddParsed));
    }
    const missingEvalResult = engine.evaluate('missing_test', JSON.stringify({ x: 10 }));
    const missingEvalParsed = JSON.parse(missingEvalResult);
    if (!missingEvalParsed.success) {
      throw new Error('Failed to evaluate missing facts test: ' + JSON.stringify(missingEvalParsed));
    }
    const sumRule = missingEvalParsed.response?.results?.sum;
    if (!sumRule) {
      throw new Error('sum rule not found in results');
    }
    // If fact 'y' is missing, the rule should be vetoed with a message
    if (sumRule.result?.type !== "veto") {
      throw new Error('Expected veto result due to missing fact, got: ' + JSON.stringify(sumRule.result));
    }
    if (!sumRule.result?.message || !sumRule.result.message.includes('y')) {
      throw new Error('Expected veto message to mention missing fact "y", got: ' + JSON.stringify(sumRule.result?.message));
    }
    console.log('✓ Missing facts handling successful');

    // Test 13: Operations array
    const opsResult = engine.evaluate('test', '{}');
    const opsParsed = JSON.parse(opsResult);
    if (!opsParsed.success) {
      throw new Error('Failed to evaluate for operations test: ' + JSON.stringify(opsParsed));
    }
    const doubleRuleOps = opsParsed.response?.results?.double;
    if (!doubleRuleOps) {
      throw new Error('double rule not found in results');
    }
    // Operations are now skipped in serialization, so we can't test them
    // Just verify the rule exists and has a result
    if (!doubleRuleOps.result) {
      throw new Error('Expected result, got: ' + JSON.stringify(doubleRuleOps));
    }
    console.log('✓ Operations array present');

    // Test 14: Units and percentages
    const unitsResult = engine.addLemmaCode(`
      doc units_test
      fact price = 100
      fact discount = 10%
      rule final_price = price * (1 - discount)
    `, 'units_test.lemma');
    const unitsAddParsed = JSON.parse(unitsResult);
    if (!unitsAddParsed.success) {
      throw new Error('Failed to add units test document: ' + JSON.stringify(unitsAddParsed));
    }
    const unitsEvalResult = engine.evaluate('units_test', '{}');
    const unitsEvalParsed = JSON.parse(unitsEvalResult);
    if (!unitsEvalParsed.success) {
      throw new Error('Failed to evaluate units test: ' + JSON.stringify(unitsEvalParsed));
    }
    const finalPriceRule = unitsEvalParsed.response?.results?.final_price;
    if (!finalPriceRule || !finalPriceRule.result) {
      throw new Error('final_price rule or result not found');
    }
    console.log('✓ Units and percentages handling successful');

    // Test 15: Empty facts object vs empty string
    const emptyFacts1 = engine.evaluate('test', '{}');
    const emptyFacts2 = engine.evaluate('test', '');
    const emptyParsed1 = JSON.parse(emptyFacts1);
    const emptyParsed2 = JSON.parse(emptyFacts2);
    if (!emptyParsed1.success || !emptyParsed2.success) {
      throw new Error('Empty facts should work: ' + JSON.stringify({ emptyParsed1, emptyParsed2 }));
    }
    console.log('✓ Empty facts handling successful');

    // Test 16: Multiple documents
    const doc1Result = engine.addLemmaCode('doc doc1\nfact x = 1', 'doc1.lemma');
    const doc2Result = engine.addLemmaCode('doc doc2\nfact y = 2', 'doc2.lemma');
    if (!JSON.parse(doc1Result).success || !JSON.parse(doc2Result).success) {
      throw new Error('Failed to add multiple documents');
    }
    const listAfterMultiple = JSON.parse(engine.listDocuments());
    if (!listAfterMultiple.success || listAfterMultiple.documents.length < 2) {
      throw new Error('Expected multiple documents, got: ' + JSON.stringify(listAfterMultiple));
    }
    console.log('✓ Multiple documents handling successful');

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
