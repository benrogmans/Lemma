/**
 * esbuild plugin for @lemmabase/lemma-engine.
 *
 * Rewrites root imports to the IIFE-safe entry so users do not have
 * to handle WASM asset paths manually in esbuild-based builds.
 */
import { createRequire } from "node:module";

export function lemmaEngineEsbuildPlugin() {
  const require = createRequire(import.meta.url);

  return {
    name: "lemma-engine",
    setup(build) {
      build.onResolve({ filter: /^@benrogmans\/lemma-engine$/ }, () => {
        const iifePath = require.resolve("@lemmabase/lemma-engine/iife");
        return { path: iifePath };
      });
    },
  };
}
