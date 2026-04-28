import type { Engine } from './lemma.bindings.js';
export { Engine, initSync } from './lemma.bindings.js';
export declare function init(): Promise<void>;
export declare function Lemma(): Promise<Engine>;

/**
 * Source location attached to an {@link EngineError}. Line and column are
 * 1-based; `length` is the UTF-8 byte length of the offending span.
 */
export interface EngineErrorSource {
  attribute: string;
  line: number;
  column: number;
  length: number;
}

/**
 * Structured error thrown by {@link Engine.run}, {@link Engine.schema},
 * {@link Engine.format}, and rejected from {@link Engine.load} (as an array).
 *
 * - `kind` classifies the failure ("parsing" for syntax, "validation" for
 *   semantic/planning including bad data values, "request" for bad API input,
 *   etc.).
 * - `message` is the inner reason only. Callers that previously parsed
 *   `"Failed to parse data 'X' as Y: ..."` strings should now use `related_data`
 *   for attribution and `message` for the reason.
 * - `related_data` is non-null when the error is attributable to a specific data
 *   input declared by the spec (e.g. a field-level form validation failure).
 * - `source` points at the offending range in the original Lemma source.
 */
export interface EngineError {
  kind: "parsing" | "validation" | "inversion" | "registry" | "request" | "resource_limit";
  message: string;
  related_data: string | null;
  spec: string | null;
  related_spec: string | null;
  source: EngineErrorSource | null;
  suggestion: string | null;
}

// ---------------------------------------------------------------------------
// Schema envelope (return shape of Engine.schema and Engine.list entries)
// ---------------------------------------------------------------------------

/** Literal value produced by `JSON.stringify` on a Lemma `LiteralValue`. */
export type LiteralValue = unknown;

/** Extension classification serialized on every {@link LemmaType}. */
export type TypeExtends =
  | "primitive"
  | {
      parent: string;
      family: string;
      defining_spec: unknown;
    };

export interface UnitDef { name: string; value: string }
export interface RatioUnitDef { name: string; value: string }

/** Discriminated union over the 10 Lemma type kinds. Field `kind` is the
 *  serde tag; kind-specific fields sit at the top level next to `kind`,
 *  `name`, and `extends`. */
export type LemmaType =
  & { name: string | null; extends: TypeExtends }
  & (
    | { kind: "boolean"; help: string }
    | {
        kind: "scale";
        minimum: string | null;
        maximum: string | null;
        decimals: number | null;
        precision: string | null;
        units: UnitDef[];
        help: string;
      }
    | {
        kind: "number";
        minimum: string | null;
        maximum: string | null;
        decimals: number | null;
        precision: string | null;
        help: string;
      }
    | {
        kind: "ratio";
        minimum: string | null;
        maximum: string | null;
        decimals: number | null;
        units: RatioUnitDef[];
        help: string;
      }
    | {
        kind: "text";
        minimum: number | null;
        maximum: number | null;
        length: number | null;
        options: string[];
        help: string;
      }
    | { kind: "date"; minimum: string | null; maximum: string | null; help: string }
    | { kind: "time"; minimum: string | null; maximum: string | null; help: string }
    | { kind: "duration"; help: string }
    | { kind: "veto"; message: string | null }
  );

/** One input on a spec. `default` is omitted (not `null`) when absent. */
export interface DataEntry {
  type: LemmaType;
  default?: LiteralValue;
}

/** Return shape of {@link Engine.schema}. */
export interface SpecSchema {
  spec: string;
  data: Record<string, DataEntry>;
  rules: Record<string, LemmaType>;
  meta: Record<string, unknown>;
}

/** One row of {@link Engine.list}. The schema is always inlined so callers
 *  never need a second `engine.schema(name, effective_from)` round-trip. */
export interface SpecListEntry {
  name: string;
  effective_from: string | null;
  effective_to: string | null;
  schema: SpecSchema;
}
