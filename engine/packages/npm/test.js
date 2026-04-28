#!/usr/bin/env node
/**
 * Node: initSync + Engine. Browser: init + Engine.
 */

import { readFileSync, existsSync } from 'fs';
import { join, dirname, resolve } from 'path';
import { fileURLToPath, pathToFileURL } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const DIST_PATH = join(__dirname, 'dist');

function assert(cond, msg) {
  if (!cond) throw new Error(msg || 'assertion failed');
}

function opIsVeto(op) {
  return op && Object.prototype.hasOwnProperty.call(op, 'veto');
}

function opIsValue(op) {
  return op && Object.prototype.hasOwnProperty.call(op, 'value');
}

function literalPrimitiveType(lit) {
  if (!lit || !lit.value || typeof lit.value !== 'object') return null;
  const keys = Object.keys(lit.value);
  return keys.length === 1 ? keys[0] : null;
}

function literalNumberValue(lit) {
  const t = literalPrimitiveType(lit);
  if (t !== 'number') return null;
  const v = lit.value.number;
  return typeof v === 'string' ? Number(v) : v;
}

function literalScaleValue(lit) {
  const t = literalPrimitiveType(lit);
  if (t !== 'scale') return null;
  const v = lit.value.scale;
  if (!Array.isArray(v) || v.length !== 2) return null;
  const amount = typeof v[0] === 'string' ? Number(v[0]) : v[0];
  const unit = v[1];
  if (typeof unit !== 'string') return null;
  return { amount, unit };
}

function formatReject(e) {
  if (Array.isArray(e)) {
    return e.map((it) => (it && it.message) ? it.message : String(it)).join('\n');
  }
  if (e && typeof e === 'object' && typeof e.message === 'string') return e.message;
  return String(e);
}

function runEx(engine, spec, rules, data, effective) {
  try {
    return engine.run(spec, rules, data, effective ?? null);
  } catch (e) {
    throw new Error(formatReject(e));
  }
}

const ERROR_KINDS = new Set([
  'parsing',
  'validation',
  'inversion',
  'registry',
  'request',
  'resource_limit',
]);

function assertEngineError(e) {
  assert(e && typeof e === 'object' && !Array.isArray(e), 'EngineError must be plain object');
  assert(ERROR_KINDS.has(e.kind), `kind must be known, got: ${e.kind}`);
  assert(typeof e.message === 'string', 'message must be string');
  assert(e.related_data === null || typeof e.related_data === 'string', 'related_data string|null');
  assert(e.spec === null || typeof e.spec === 'string', 'spec string|null');
  assert(e.source === null || (e.source && typeof e.source === 'object'), 'source object|null');
}

function assertResponseShape(resp, specName) {
  assert(resp && typeof resp === 'object', 'run() must return object');
  assert(resp.spec_name === specName, `spec_name want ${specName}, got ${resp.spec_name}`);
  assert(
    resp.results && typeof resp.results === 'object' && !Array.isArray(resp.results),
    'results must be plain object'
  );
  assert(Array.isArray(resp.data), 'data must be array');
}

async function case_(name, fn) {
  const t0 = performance.now();
  try {
    await fn();
    console.log(`  ok  ${name} (${(performance.now() - t0).toFixed(1)}ms)`);
  } catch (e) {
    console.error(`  FAIL ${name}:`, e.message || e);
    throw e;
  }
}

function specNames(listed) {
  return listed.map((e) => (typeof e === 'string' ? e : e && e.name)).filter(Boolean);
}

export async function test() {
  console.log('Lemma WASM package tests\n');

  if (!existsSync(join(DIST_PATH, 'lemma.js'))) {
    console.error('dist/ missing. Run: node engine/packages/npm/build.js');
    process.exit(1);
  }

  const importRegex = /from\s+['"](\.[^'"]+)['"]/g;
  const pkgJson = JSON.parse(readFileSync(join(DIST_PATH, 'package.json'), 'utf-8'));
  const publishedFiles = pkgJson.files || [];
  for (const entry of ['lemma.js', 'lsp.js']) {
    const src = readFileSync(join(DIST_PATH, entry), 'utf-8');
    let match;
    importRegex.lastIndex = 0;
    while ((match = importRegex.exec(src)) !== null) {
      const target = join(DIST_PATH, match[1]);
      if (!existsSync(target)) {
        throw new Error(`${entry} imports '${match[1]}' but file missing`);
      }
      const rel = match[1].replace(/^\.\//, '');
      if (!publishedFiles.some((f) => rel === f || rel.startsWith(f + '/'))) {
        throw new Error(`${entry}: '${match[1]}' not in package.json files`);
      }
    }
  }
  for (const entry of publishedFiles) {
    if (!existsSync(join(DIST_PATH, entry))) {
      throw new Error(`package.json lists "${entry}" but missing in dist/`);
    }
  }
  console.log('  ok  package graph (imports + npm files)\n');

  const { initSync, Engine } = await import(join(DIST_PATH, 'lemma.js'));
  initSync({ module: readFileSync(join(DIST_PATH, 'lemma_bg.wasm')) });
  console.log('  ok  initSync + Engine\n');

  const engine = new Engine();
  let passed = 0;

  const run = async (title, fn) => {
    await case_(title, fn);
    passed++;
  };

  try {
    await run('load + run shape + double rule', async () => {
      await engine.load(
        `spec test
      data x: 10
      rule double: x * 2`,
        'test.lemma'
      );
      const r = runEx(engine, 'test', [], {}, null);
      assertResponseShape(r, 'test');
      assert(Object.keys(r.results).includes('double'), `keys: ${Object.keys(r.results)}`);
      assert(opIsValue(r.results.double.result), 'double Value');
      assert(literalNumberValue(r.results.double.result.value) === 20, 'double=20');
    });

    await run('list includes test spec and inlines schema', async () => {
      const listed = engine.list();
      assert(Array.isArray(listed) && listed.length >= 1, `list: ${JSON.stringify(listed)}`);
      assert(specNames(listed).includes('test'), `names: ${specNames(listed)}`);
      const entry = listed.find((e) => e.name === 'test');
      assert(entry.schema && entry.schema.spec === 'test', 'list entry carries its schema inline');
      assert(Object.keys(entry.schema.data).includes('x'), 'inlined schema preserves data keys');
    });

    await run('schema → spec/data/rules with DataEntry + flat type', async () => {
      const schema = engine.schema('test', null);
      assert(schema.spec === 'test');
      assert(schema.data && typeof schema.data === 'object');
      assert(schema.rules && typeof schema.rules === 'object');
      assert(Object.keys(schema.data).includes('x'));
      assert(Object.keys(schema.rules).includes('double'));
      const x = schema.data.x;
      assert(x && typeof x === 'object' && !Array.isArray(x), 'DataEntry is a named object');
      assert(x.type && typeof x.type.kind === 'string', 'type carries `kind` discriminator');
      const doubleRule = schema.rules.double;
      assert(typeof doubleRule.kind === 'string', 'rule types expose `kind` at the top level');
    });

    await run('run rule filter', async () => {
      const r = runEx(engine, 'test', ['double'], {}, null);
      assert(Object.keys(r.results).length === 1 && r.results.double, 'filtered');
    });

    await run('format()', async () => {
      const out = engine.format('spec fmt\ndata a: 1\nrule r: a', null);
      assert(typeof out === 'string' && out.includes('spec fmt'));
    });

    await run('data overrides', async () => {
      await engine.load(
        `spec type_test
      data number_data: 42
      data bool_data: false
      data string_data: "hello"
      data unit_data: 100
      data date_data: 2024-01-15
      rule double_number: number_data * 2`,
        'type_test.lemma'
      );
      const r = runEx(
        engine,
        'type_test',
        [],
        {
          number_data: 50,
          bool_data: true,
          string_data: 'world',
          unit_data: '200',
          date_data: '2024-12-25',
        },
        null
      );
      assert(literalNumberValue(r.results.double_number.result.value) === 100);
    });

    await run('load parse errors as JsError array', async () => {
      let threw = false;
      try {
        await engine.load('spec invalid\ndata x :', 'bad.lemma');
      } catch (e) {
        threw = true;
        assert(Array.isArray(e), 'load rejection must be array of EngineError');
        assert(e.length >= 1);
        for (const err of e) assertEngineError(err);
        assert(e.some((err) => err.kind === 'parsing'), 'expected at least one parsing error');
      }
      assert(threw);
    });

    await run('run structured error attributes data', async () => {
      await engine.load(
        `spec bridge
      data bridge_height: scale -> unit meter 1.0
      rule span: bridge_height`,
        'workspace.lemma'
      );
      let threw = false;
      try {
        engine.run('bridge', [], { bridge_height: '4 mete' }, null);
      } catch (e) {
        threw = true;
        assertEngineError(e);
        assert(e.kind === 'validation', `kind=${e.kind}`);
        assert(e.related_data === 'bridge_height', `related_data=${e.related_data}`);
        assert(e.message.startsWith('Unknown unit'), `message=${e.message}`);
        assert(e.source && typeof e.source.line === 'number', 'source has line');
        assert(typeof e.source.column === 'number');
        assert(typeof e.source.length === 'number');
      }
      assert(threw);
    });

    await run('run missing spec', async () => {
      let threw = false;
      try {
        runEx(engine, '__nope__', [], {}, null);
      } catch {
        threw = true;
      }
      assert(threw);
    });

    await run('data_values not object', async () => {
      let threw = false;
      try {
        engine.run('test', [], 'not-an-object', null);
      } catch {
        threw = true;
      }
      assert(threw);
    });

    await run('veto sqrt(-1)', async () => {
      await engine.load(
        `spec veto_test
      data x: 10
      rule bad_sqrt: sqrt(-1)`,
        'veto.lemma'
      );
      const r = runEx(engine, 'veto_test', [], {}, null);
      assert(opIsVeto(r.results.bad_sqrt.result));
    });

    await run('missing data veto', async () => {
      await engine.load(
        `spec missing_test
      data x: number
      data y: number
      rule sum: x + y`,
        'miss.lemma'
      );
      const r = runEx(engine, 'missing_test', [], { x: 10 }, null);
      assert(opIsVeto(r.results.sum.result));
      assert(String(r.results.sum.result.veto).includes('y'));
    });

    await run('scale eur→usd', async () => {
      await engine.load(
        `spec scale_conv
      data money: scale
        -> unit eur 1
        -> unit usd 1.19
      rule price_usd: 100 eur in usd`,
        'sc.lemma'
      );
      const r = runEx(engine, 'scale_conv', [], {}, null);
      const sc = literalScaleValue(r.results.price_usd.result.value);
      assert(sc && sc.unit === 'usd' && sc.amount === 119);
    });

    await run('multiple specs', async () => {
      await engine.load('spec spec1\ndata x: 1', 's1.lemma');
      await engine.load('spec spec2\ndata y: 2', 's2.lemma');
      assert(specNames(engine.list()).length >= 2);
    });

    console.log(`\nAll ${passed} cases passed.`);
  } catch {
    console.error('\nRebuild: node engine/packages/npm/build.js');
    process.exit(1);
  }
}

const isMain =
  process.argv[1] &&
  import.meta.url === pathToFileURL(resolve(process.argv[1])).href;
if (isMain) await test();
