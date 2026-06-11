import type { Engine } from './lemma.bindings.js';
export { Engine, initSync } from './lemma.bindings.js';
export declare function init(): Promise<void>;
export declare function Lemma(): Promise<Engine>;

/** Resolved shape of {@link Engine.fetch}. */
export interface RegistryFetchResult {
  source: string;
  id: string;
}

declare module './lemma.bindings.js' {
  interface Engine {
    /**
     * Load multiple Lemma sources in one planning pass. Object keys become error-reporting
     * paths (`SourceType::Path`); use `""` for volatile/inline. Non-empty `dependency` tags
     * the batch as that dependency id. Throws `EngineError[]` on failure.
     */
    load_batch(
      sources: Record<string, string>,
      dependency?: string | null,
    ): void;

    /**
     * Download Lemma source from the registry for `name` (e.g. `@org/pkg`). Resolves with
     * `{ source, id }`; does not load the engine. Rejects with `EngineError[]` like `load`.
     */
    fetch(name: string): Promise<RegistryFetchResult>;

    /**
     * JSON serialization of `Vec<ResolvedRepository>` from [`Engine::list`]:
     * each item has `repository` ([`LemmaRepository`]) and `specs` (`LemmaSpecSet[]`),
     * each set has `repository`, `name`, and `specs` (`LemmaSpec[]`).
     */
    list(): ResolvedRepositoryJson[];

    /**
     * Formatted Lemma source for a loaded repository (from in-engine AST). Use `"lemma"` for embedded units stdlib.
     */
    format_repository(repository: string): string;

    /**
     * `repository`: qualifier or `null`/omit for workspace — same as `Engine::schema` `repo`.
     */
    schema(
      repository: string | null | undefined,
      spec: string,
      effective?: string | null,
    ): SpecSchema;

    /**
     * `repository`: qualifier or `null`/omit for workspace — same as `Engine::run` `repo`.
     */
    run(
      repository: string | null | undefined,
      spec: string,
      rule_names: string[] | string,
      data_values: Record<string, unknown>,
      effective?: string | null,
      explain?: boolean,
    ): EvaluationResponse;
  }
}

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
 * {@link Engine.format}, {@link Engine.load}, and {@link Engine.load_batch}
 * (as an array), and rejected from {@link Engine.fetch} (as an array).
 *
 * - `kind` classifies the failure ("parsing" for syntax, "validation" for
 *   semantic/planning including bad data values, "missing_repository" when a
 *   referenced repo is not loaded, "request" for bad API input, etc.).
 * - `message` is the inner reason only. Callers that previously parsed
 *   `"Failed to parse data 'X' as Y: ..."` strings should now use `related_data`
 *   for attribution and `message` for the reason.
 * - `related_data` is non-null when the error is attributable to a specific data
 *   input declared by the spec (e.g. a field-level form validation failure).
 * - `source` points at the offending range in the original Lemma source.
 */
export interface EngineError {
  kind:
    | "parsing"
    | "validation"
    | "inversion"
    | "registry"
    | "missing_repository"
    | "request"
    | "resource_limit";
  message: string;
  related_data: string | null;
  spec: string | null;
  related_spec: string | null;
  source: EngineErrorSource | null;
  suggestion: string | null;
  /** Present for `missing_repository` and `registry` errors (`@…` id). */
  repository: string | null;
}

// ---------------------------------------------------------------------------
// Schema envelope (return shape of Engine.schema)
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

export interface UnitDef {
  name: string;
  factor: { numer: string; denom: string };
  minimum?: string | null;
  maximum?: string | null;
  default?: string | null;
}

export interface RatioUnitDef {
  name: string;
  value: { numer: string; denom: string };
  minimum?: string | null;
  maximum?: string | null;
  default?: string | null;
}

/** Discriminated union over the 10 Lemma type kinds. Field `kind` is the
 *  serde tag; kind-specific fields sit at the top level next to `kind`,
 *  `name`, and `extends`. */
export type LemmaType =
  & { name: string | null; extends: TypeExtends }
  & (
    | { kind: "boolean"; help: string }
    | {
        kind: "quantity";
        minimum: string | null;
        maximum: string | null;
        decimals: number | null;
        units: UnitDef[];
        help: string;
      }
    | {
        kind: "quantity range";
        minimum: string | null;
        maximum: string | null;
        decimals: number | null;
        units: UnitDef[];
        help: string;
      }
    | {
        kind: "number";
        minimum: string | null;
        maximum: string | null;
        decimals: number | null;
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
        kind: "ratio range";
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
    | { kind: "veto"; message: string | null }
  );

/** One input declared in a spec. Omitted fields are absent (not `null`). */
export interface DataEntry {
  type: LemmaType;
  /** Literal bound in the source (`data x: literal`). */
  bound_value?: LiteralValue;
  /** `-> default ...` suggestion; omitted from `bound_value` until evaluation applies it. */
  default?: LiteralValue;
}

/** Return shape of {@link Engine.run}. */
export interface EvaluationResponse {
  spec: string;
  effective: string;
  results: Record<string, RuleResult>;
  data: EvaluationDataEntry[];
}

export interface RuleResult {
  vetoed: boolean;
  display?: string | null;
  veto_reason?: string | null;
  rule_type: string;
  quantity?: Record<string, string> | null;
  ratio?: Record<string, string> | null;
  number?: string | null;
  boolean?: boolean | null;
  text?: string | null;
  date?: unknown | null;
  time?: unknown | null;
  calendar?: { value: string; unit: string } | null;
  range?: { from: RuleResultPayload; to: RuleResultPayload } | null;
  explanation?: unknown | null;
}

export interface RuleResultPayload {
  quantity?: Record<string, string> | null;
  ratio?: Record<string, string> | null;
  number?: string | null;
  boolean?: boolean | null;
  text?: string | null;
  date?: unknown | null;
  time?: unknown | null;
  calendar?: { value: string; unit: string } | null;
}

export interface EvaluationDataEntry {
  path: string;
  value: unknown;
}

/** Return shape of {@link Engine.schema}. */
export interface SpecSchema {
  spec: string;
  data: Record<string, DataEntry>;
  /** Rule result types; quantity and ratio entries expose `units[]` like their data counterparts. */
  rules: Record<string, LemmaType>;
  meta: Record<string, unknown>;
}

/** JSON mirror of Rust `ResolvedRepository` (engine `list`). */
export interface ResolvedRepositoryJson {
  repository: LemmaRepositoryJson;
  /** [`LemmaSpecSet`] list for this resolved repository. */
  specs: LemmaSpecSetJson[];
}

/** JSON mirror of Rust `LemmaSpecSet` as serialized by the engine. */
export interface LemmaSpecSetJson {
  repository: LemmaRepositoryJson;
  name: string;
  /** Temporal versions, ascending `effective_from` (same order as `iter_specs`). */
  specs: LemmaSpecJson[];
}

/** JSON mirror of Rust `LemmaRepository`. */
export interface LemmaRepositoryJson {
  name: string | null;
  dependency: string | null;
  start_line: number;
  source_type: unknown;
}

/** JSON mirror of Rust `EffectiveDate` (externally tagged). */
export type EffectiveDateJson =
  | { Origin: null }
  | { DateTimeValue: DateTimeValueJson };

/** JSON mirror of Rust `DateTimeValue`. */
export interface DateTimeValueJson {
  year: number;
  month: number;
  day: number;
  hour: number;
  minute: number;
  second: number;
  microsecond: number;
  timezone: unknown;
}

/** JSON mirror of Rust `LemmaSpec` (full AST; deep nodes are engine-shaped). */
export interface LemmaSpecJson {
  name: string;
  effective_from: EffectiveDateJson;
  source_type: unknown;
  start_line: number;
  commentary: string | null;
  data: unknown[];
  rules: unknown[];
  meta_fields: unknown[];
}
