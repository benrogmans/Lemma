# @benrogmans/lemma-engine

Lemma is a declarative programming language for expressing business rules. This WebAssembly build for JavaScript and TypeScript provides an embeddable runtime, so that Lemma code can be evaluated anywhere.

**New to Lemma?** Check out the [language introduction](https://github.com/benrogmans/lemma#quick-start) and [examples](https://github.com/benrogmans/lemma/tree/main/docs/examples).

## Installation

```bash
npm install @benrogmans/lemma-engine
```

## JavaScript API Reference

### Initialization

#### Browser
```javascript
import init, { WasmEngine } from '@benrogmans/lemma-engine';

// Async initialization (recommended for browsers)
await init();
const engine = new WasmEngine();
```

#### Node.js
```javascript
import { readFileSync } from 'fs';
import { WasmEngine, initSync } from '@benrogmans/lemma-engine';

// Synchronous initialization for Node.js
const wasmBytes = readFileSync('node_modules/@benrogmans/lemma-engine/lemma_bg.wasm');
initSync(wasmBytes);
const engine = new WasmEngine();
```

#### Bundlers (Webpack, Vite, etc.)
```javascript
import init, { WasmEngine } from '@benrogmans/lemma-engine';
import wasmUrl from '@benrogmans/lemma-engine/lemma_bg.wasm?url';

// Initialize with URL
await init(wasmUrl);
const engine = new WasmEngine();
```

### Core Methods

#### `addLemmaCode(code: string, filename: string): string`

Adds a Lemma document to the engine.

```javascript
const result = engine.addLemmaCode(`
  doc employee_contract

  fact salary = 5000 eur/month
  fact start_date = 2024-01-15
  fact vacation_days = 25

  rule annual_salary = salary * 12
  rule daily_rate = salary / 21
`, 'employee.lemma');

const response = JSON.parse(result);
if (response.success) {
  console.log('Document loaded:', response.data);
} else {
  console.error('Error:', response.error);
}
```

#### `evaluate(docName: string, facts: string): string`

Evaluates a document with optional runtime facts.

```javascript
// Evaluate with default facts
const result1 = engine.evaluate('employee_contract', '{}');

// Evaluate with runtime fact overrides (as JSON object)
const result2 = engine.evaluate('employee_contract', JSON.stringify({
  salary: 6000,
  vacation_days: 30
}));

const response = JSON.parse(result2);
if (response.success) {
  console.log('Document:', response.data.document);
  console.log('Rules:', response.data.rules);
  // Access specific rule results directly:
  // response.data.rules.annual_salary.value
}
```

#### `listDocuments(): string`

Lists all loaded documents in the engine.

```javascript
const result = engine.listDocuments();
const response = JSON.parse(result);

if (response.success) {
  console.log('Loaded documents:', response.data);
  // response.data is an array of document names
}
```

### Response Format

All methods return a JSON string with this structure:

```typescript
interface WasmResponse {
  success: boolean;
  data?: any;        // Success data (varies by method)
  error?: string;    // Error message if success is false
  warnings?: string[]; // Optional warnings
}
```

### Complete Example

```javascript
import init, { WasmEngine } from '@benrogmans/lemma-engine';

async function calculatePricing() {
  // Initialize WASM
  await init();
  const engine = new WasmEngine();

  // Define pricing rules
  const pricingDoc = `
    doc product_pricing

    fact base_price = 100 usd
    fact quantity = 1
    fact is_member = false
    fact promo_code = ""

    rule bulk_discount = 0%
      unless quantity >= 10 then 5%
      unless quantity >= 50 then 10%
      unless quantity >= 100 then 15%

    rule member_discount = 0%
      unless is_member then 10%

    rule promo_discount = 0%
      unless promo_code == "SAVE20" then 20%
      unless promo_code == "SAVE10" then 10%

    rule best_discount = max(bulk_discount, member_discount, promo_discount)
    rule final_price = base_price * quantity * (1 - best_discount)
  `;

  // Load the document
  const loadResult = JSON.parse(
    engine.addLemmaCode(pricingDoc, 'pricing.lemma')
  );

  if (!loadResult.success) {
    throw new Error(loadResult.error);
  }

    // Calculate for different scenarios
    const scenarios = [
      { quantity: 25, is_member: false, promo_code: "" },
      { quantity: 10, is_member: true, promo_code: "" },
      { quantity: 5, is_member: false, promo_code: "SAVE20" }
    ];

    for (const scenario of scenarios) {
      const result = JSON.parse(
        engine.evaluate('product_pricing', JSON.stringify(scenario))
      );

    if (result.success) {
      console.log(`Scenario:`, scenario);
      // Access rule results directly by name
      const finalPrice = result.data.rules.final_price.value;
      const bestDiscount = result.data.rules.best_discount.value;
      console.log(`Final price:`, finalPrice);
      console.log(`Discount applied:`, bestDiscount);
      console.log('---');
    }
  }
}

calculatePricing().catch(console.error);
```

### TypeScript Support

The package includes TypeScript definitions. For better type safety:

```typescript
import init, { WasmEngine } from '@benrogmans/lemma-engine';

interface PricingFacts {
  quantity: number;
  is_member: boolean;
  promo_code: string;
}

interface PricingResults {
  base_price: string;
  final_price: string;
  best_discount: string;
  // ... other fields
}

interface EvaluationResponse {
  success: boolean;
  data?: {
    document: string;
    rules: {
      [ruleName: string]: {
        value: any;  // The computed value (e.g., {Number: "100"}, {Unit: "50 EUR"})
        veto?: string;  // Present if rule was vetoed
        missing_facts?: string[];  // Present if rule couldn't be evaluated
        operations?: Array<{  // Operation records (always present if rule was evaluated)
          type: string;  // "fact_used", "operation_executed", "final_result", etc.
          // Additional fields depend on type
        }>;
      };
    };
  };
  error?: string;
  warnings?: string[];  // Document-level warnings
}

async function typedExample() {
  await init();
  const engine = new WasmEngine();

  // ... load document

  const facts = {
    quantity: 10,
    is_member: true,
    promo_code: "SAVE10"
  };

  const result: EvaluationResponse = JSON.parse(
    engine.evaluate('product_pricing', JSON.stringify(facts))
  );

  if (result.success && result.data) {
    const price = result.data.rules.final_price?.value;
    // price might be {Unit: "100 USD"} or {Number: "100"}
    // depending on the rule's result type
  }
}
```

### Error Handling

```javascript
try {
  const result = JSON.parse(
    engine.addLemmaCode('invalid syntax', 'bad.lemma')
  );

  if (!result.success) {
    console.error('Lemma error:', result.error);
    // Handle parse/semantic errors
  }
} catch (e) {
  // Handle JSON parse errors or WASM exceptions
  console.error('System error:', e);
}
```

### Performance Tips

1. **Initialize once**: The WASM module should be initialized once per application
2. **Reuse engines**: Create one `WasmEngine` and load multiple documents
3. **Batch operations**: Load all documents before evaluating
4. **Cache results**: Evaluation results can be cached if facts don't change

### Compatibility

Works in modern browsers with WebAssembly support and Node.js with ES module support.

## License

Apache-2.0

## Links

- [GitHub Repository](https://github.com/benrogmans/lemma)
- [Lemma Language Guide](https://github.com/benrogmans/lemma/tree/main/docs)
- [Examples](https://github.com/benrogmans/lemma/tree/main/docs/examples)
- [Report Issues](https://github.com/benrogmans/lemma/issues)
