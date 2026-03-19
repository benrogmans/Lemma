/**
 * @benrogmans/lemma-engine — public entry.
 * Glue: lemma.bindings.js (wasm-pack output, renamed by build).
 */
import initWasm, * as bindings from './lemma.bindings.js';

let wasmReady = null;

export async function init() {
  if (wasmReady === null) wasmReady = initWasm();
  return wasmReady;
}

export function initSync(module) {
  return bindings.initSync(module);
}

export async function Lemma() {
  await init();
  return new bindings.Engine();
}

export const Engine = bindings.Engine;
