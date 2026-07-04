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

function ruleNumber(rr) {
  if (!rr || rr.number == null) return null;
  return Number(rr.number);
}

function ruleMeasureUnit(rr, unit) {
  if (!rr || !rr.measure || typeof rr.measure !== 'object') return null;
  const v = rr.measure[unit];
  return v != null ? Number(v) : null;
}

function runEx(engine, spec, rules, data, effective) {
  try {
    return engine.run(null, spec, rules, data, effective ?? null);
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

function formatReject(e) {
  if (Array.isArray(e)) {
    return e.map((it) => (it && it.message) ? it.message : String(it)).join('\n');
  }
  if (e && typeof e === 'object' && typeof e.message === 'string') return e.message;
  return String(e);
}

function assertResponseShape(resp, specName) {
  assert(resp && typeof resp === 'object', 'run() must return object');
  assert(resp.spec === specName, `spec want ${specName}, got ${resp.spec}`);
  assert(typeof resp.effective === 'string', 'effective must be string');
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

function flattenListGroups(groups) {
  const out = [];
  for (const g of groups) {
    const repository = g.repository;
    for (const specSet of g.specs) {
      for (const lemmaSpec of specSet.specs) {
        out.push({ ...lemmaSpec, repository });
      }
    }
  }
  return out;
}

function lemmaSpecSourcePathIncludes(lemmaSpec, needle) {
  const st =
    lemmaSpec && typeof lemmaSpec === 'object' && lemmaSpec.source_type != null
      ? lemmaSpec.source_type
      : null;
  if (st == null) return false;
  const s = JSON.stringify(st);
  return typeof s === 'string' && s.includes(needle);
}

function specNames(listGroups) {
  return flattenListGroups(listGroups)
    .map((e) => e && e.name)
    .filter(Boolean);
}

function failTest(err) {
  if (err?.message) {
    console.error(err.message);
  }
  console.error('\nRebuild: node engine/packages/npm/build.js');
  process.exit(1);
}

export async function test() {
  console.log('Lemma WASM package tests\n');

  try {
  if (!existsSync(join(DIST_PATH, 'lemma.js'))) {
    throw new Error('dist/ missing. Run: node engine/packages/npm/build.js');
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

    await run('embedded lemma in list + format_repository', () => {
      const fresh = new Engine();
      const groups = fresh.list();
      assert(
        groups.some((g) => g.repository && g.repository.name === 'lemma'),
        `expected lemma in list: ${JSON.stringify(groups.map((g) => g.repository?.name))}`
      );
      const src = fresh.format_repository('lemma');
      assert(src.includes('spec units'), 'format_repository must include spec units');
      assert(src.includes('trait duration'), 'format_repository must include duration typedef');
    });

    await run('load + run shape + double rule', () => {
      engine.load(
        `spec test
      data x: 10
      rule double: x * 2`,
        'test.lemma'
      );
      const r = runEx(engine, 'test', null, {}, null);
      assertResponseShape(r, 'test');
      assert(Object.keys(r.results).includes('double'), `keys: ${Object.keys(r.results)}`);
      assert(!r.results.double.vetoed, 'double not vetoed');
      assert(ruleNumber(r.results.double) === 20, 'double=20');
    });

    await run('list + source fields; schema via Engine.schema', () => {
      const groups = engine.list();
      assert(Array.isArray(groups) && groups.length >= 1, `list: ${JSON.stringify(groups)}`);
      const flat = flattenListGroups(groups);
      assert(flat.some((r) => r.name === 'test'), `names: ${specNames(groups)}`);
      const testRow = flat.find((r) => r.name === 'test');
      assert(
        typeof testRow.start_line === 'number' && testRow.start_line >= 1,
        'spec start_line'
      );
      assert(lemmaSpecSourcePathIncludes(testRow, 'test.lemma'), 'spec source_type path');
      const repo = testRow.repository;
      assert(typeof repo.start_line === 'number');
      assert(!('schema' in testRow), 'catalog row must not inline schema');
      const schema = engine.schema(null, 'test', null);
      assert(schema.spec === 'test');
      assert(Object.keys(schema.data).includes('x'));
    });

    await run('schema → spec/data/rules with DataEntry + flat type', () => {
      const schema = engine.schema(null, 'test', null);
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

    await run('schema rule result units for measure and ratio', () => {
      engine.load(
        `spec units_contract
data money: measure -> unit eur 1 -> unit usd 0.91
data rate: ratio
  -> unit basis_points 10000
  -> unit percent 100
  -> default 500 basis_points
rule total: money
rule rate_out: rate`,
        'units_contract.lemma'
      );
      const schema = engine.schema(null, 'units_contract', null);
      assert(Array.isArray(schema.rules.total.units) && schema.rules.total.units.length >= 1);
      assert(schema.rules.total.units[0].factor, 'measure rule units expose factor');
      assert(Array.isArray(schema.rules.rate_out.units) && schema.rules.rate_out.units.length >= 1);
      assert(schema.rules.rate_out.units[0].value, 'ratio rule units expose value');
    });

    await run('run rule filter', () => {
      const r = runEx(engine, 'test', ['double'], {}, null);
      assert(Object.keys(r.results).length === 1 && r.results.double, 'filtered');
    });

    await run('format()', () => {
      const out = engine.format('spec fmt\ndata a: 1\nrule r: a', null);
      assert(typeof out === 'string' && out.includes('spec fmt'));
    });

    await run('data overrides', () => {
      engine.load(
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
        null,
        {
          number_data: 50,
          bool_data: true,
          string_data: 'world',
          unit_data: '200',
          date_data: '2024-12-25',
        },
        null
      );
      assert(ruleNumber(r.results.double_number) === 100);
    });

    await run('load parse errors as EngineError array', () => {
      let threw = false;
      try {
        engine.load('spec invalid\ndata x :', 'bad.lemma');
      } catch (e) {
        threw = true;
        assert(Array.isArray(e), 'load throw must be array of EngineError');
        assert(e.length >= 1);
        for (const err of e) assertEngineError(err);
        assert(e.some((err) => err.kind === 'parsing'), 'expected at least one parsing error');
      }
      assert(threw);
    });

    await run('fetch rejects empty registry id', async () => {
      let threw = false;
      try {
        await engine.fetch('   ');
      } catch (e) {
        threw = true;
        assert(Array.isArray(e), 'rejection must be array of EngineError');
        assert(e.length >= 1);
        for (const err of e) assertEngineError(err);
        assert(
          e.some((err) => err.kind === 'request'),
          'expected request error for empty id'
        );
      }
      assert(threw);
    });

    await run('invalid measure unit override completes with veto', () => {
      engine.load(
        `spec bridge
data bridge_height: measure -> unit meter 1.0
rule span: bridge_height`,
        'workspace.lemma'
      );
      const response = runEx(engine, 'bridge', null, { bridge_height: '4 mete' }, null);
      assert(response.results.span.vetoed === true, 'span must veto on unknown unit');
      assert(
        typeof response.results.span.veto_reason === 'string' &&
          response.results.span.veto_reason.includes('Unknown unit'),
        `veto_reason=${response.results.span.veto_reason}`
      );
    });

    await run('run missing spec', () => {
      let threw = false;
      try {
        runEx(engine, '__nope__', null, {}, null);
      } catch {
        threw = true;
      }
      assert(threw);
    });

    await run('data_values not object', () => {
      let threw = false;
      try {
        engine.run(null, 'test', null, 'not-an-object', null);
      } catch {
        threw = true;
      }
      assert(threw);
    });

    await run('veto sqrt(-1)', () => {
      engine.load(
        `spec veto_test
      data x: 10
      rule bad_sqrt: sqrt(-1)`,
        'veto.lemma'
      );
      const r = runEx(engine, 'veto_test', null, {}, null);
      assert(r.results.bad_sqrt.vetoed === true);
    });

    await run('invalid effective must error not default to now', () => {
      engine.load(
        `spec temporal
data x: 1
rule r: x`,
        'temporal.lemma'
      );
      let threw = false;
      try {
        runEx(engine, 'temporal', null, {}, 'not-a-datetime');
      } catch {
        threw = true;
      }
      assert(threw, 'invalid effective string must throw before planning, not fall back to now');
    });

    await run('missing data veto', () => {
      engine.load(
        `spec missing_test
      data x: number
      data y: number
      rule sum: x + y`,
        'miss.lemma'
      );
      const r = runEx(engine, 'missing_test', null, { x: 10 }, null);
      assert(r.results.sum.vetoed === true);
      assert(typeof r.results.sum.veto_reason === 'string' && r.results.sum.veto_reason.includes('y'));
    });

    await run('measure unit conversion', () => {
      // unit usd 0.84: 1 USD = 0.84 EUR (canonical). 100 usd as eur => 100 * 0.84 = 84.
      engine.load(
        `spec measure_conv
      data money: measure
        -> unit eur 1
        -> unit usd 0.84
      rule price_eur: 100 usd as eur`,
        'sc.lemma'
      );
      const r = runEx(engine, 'measure_conv', null, {}, null);
      const eur = ruleMeasureUnit(r.results.price_eur, 'eur');
      assert(eur === 84, `expected 84 eur, got ${eur}`);
    });

    await run('multiple specs', () => {
      engine.load('spec spec1\ndata x: 1', 's1.lemma');
      engine.load('spec spec2\ndata y: 2', 's2.lemma');
      assert(specNames(engine.list()).length >= 2);
    });

    console.log(`\nAll ${passed} cases passed.`);
  } catch (err) {
    failTest(err);
  }
}

const isMain =
  process.argv[1] &&
  import.meta.url === pathToFileURL(resolve(process.argv[1])).href;
if (isMain) {
  await test().catch(failTest);
}
