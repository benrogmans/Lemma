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
     * Load Lemma source(s).
     * - string → volatile workspace source
     * - object or `[label, code][]` → labeled sources in one planning pass
     * Throws `EngineError[]` on failure. `null`/`undefined` rejected.
     */
    load(code: string): void;
    load(sources: Record<string, string> | Array<[string, string]>): void;

    /**
     * Download Lemma source from the registry for `name` (e.g. `@org/pkg`). Resolves with
     * `{ source, id }`; does not load the engine. Rejects with `EngineError[]` like `load`.
     */
    fetch(name: string): Promise<RegistryFetchResult>;

    /**
     * JSON serialization of `Vec<ResolvedRepository>` from [`Engine::list`]:
     * each item has `repository` (name or null for workspace) and `specs`
     * (`ListedSpec` rows: name, effective_from, effective_to).
     */
    list(): ResolvedRepositoryJson[];

    /**
     * Spec interface and temporal window at `effective`. Lemma text is {@link Engine.source}.
     */
    show(
      repository: string | null | undefined,
      spec: string,
      effective?: string | null,
    ): Show;

    /**
     * Formatted canonical Lemma source. Omit `spec` for whole-repository text.
     */
    source(
      repository: string | null | undefined,
      spec?: string | null,
      effective?: string | null,
    ): string;

    /**
     * Remove a temporal spec slice. `effective`: ISO datetime or omit for now.
     */
    remove(
      repository: string | null | undefined,
      spec: string,
      effective?: string | null,
    ): void;

    /** Resource limits configured for this engine. */
    limits(): ResourceLimitsJson;

    /**
     * Canonical formatting of Lemma source. Throws `EngineError` on parse error.
     * `attribute` is an optional path label used in error messages.
     */
    format(code: string, attribute?: string | null): string;

    /**
     * `data_values`: pass integers as numbers, decimals as strings.
     */
    run(
      repository: string | null | undefined,
      spec: string,
      effective: string | null | undefined,
      data_values?: Record<string, unknown>,
      rule_names?: string[] | string | null,
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
 * Structured error thrown by {@link Engine.run}, {@link Engine.show},
 * {@link Engine.load}, and {@link Engine.fetch}
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
// Show envelope (return shape of Engine.show)
// ---------------------------------------------------------------------------

/** Literal value on API wire (`suggestion`, `prefilled`, committed `value`). Canonical plan storage omits `measure` / `ratio` maps. */
export interface WireLiteralValue {
  value: unknown;
  lemma_type: LemmaType;
  display_value: string;
  /** All declared measure units when the type has unit definitions. */
  measure?: Record<string, string>;
  /** All declared ratio units when the type has unit definitions. */
  ratio?: Record<string, string>;
}

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
  suggestion?: string | null;
}

export interface RatioUnitDef {
  name: string;
  value: { numer: string; denom: string };
  minimum?: string | null;
  maximum?: string | null;
  suggestion?: string | null;
}

/** Discriminated union over the 10 Lemma type kinds. Field `kind` is the
 *  serde tag; kind-specific fields sit at the top level next to `kind`,
 *  `name`, and `extends`. */
export type LemmaType =
  & { name: string | null; extends: TypeExtends }
  & (
    | { kind: "boolean"; help: string }
    | {
        kind: "measure";
        minimum: string | null;
        maximum: string | null;
        decimals: number | null;
        units: UnitDef[];
        help: string;
      }
    | {
        kind: "measure range";
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
  /** Spec literal or literal `with` binding; UIs may skip review. */
  prefilled?: WireLiteralValue;
  /** `-> suggest ...` suggestion; prompt with prefill in interactive UIs. */
  suggestion?: WireLiteralValue;
  /** Local rule names that transitively need this data (planning time). */
  needed_by_rules: string[];
}

/** Return shape of {@link Engine.run}. */
export interface EvaluationResponse {
  spec: string;
  effective: string;
  results: Record<string, RuleResult>;
}

export interface RuleResult {
  vetoed: boolean;
  display?: string | null;
  veto_reason?: string | null;
  rule_type: string;
  /** Input keys still unbound for this rule (overlay-aware; same keys as Show.data). */
  missing_data?: string[];
  measure?: Record<string, string> | null;
  ratio?: Record<string, string> | null;
  number?: string | null;
  boolean?: boolean | null;
  text?: string | null;
  date?: unknown | null;
  time?: unknown | null;
  calendar?: { value: string; unit: string } | null;
  range?: { from: RuleResultPayload; to: RuleResultPayload } | null;
  /** Present when `run(..., explain: true)`. Shape: documentation/schemas/explanation.v1.json */
  explanation?: Explanation | null;
}

/** One evaluated unless condition, stated as a fact. */
export interface ExplanationCause {
  condition: string;
  value: string;
  children?: ExplanationNode[];
}

export interface ExplanationConversionStep {
  role: "outcome" | "rule" | "source";
  text: string;
}

/** Nested explanation tree node (tagged by `type`). */
export type ExplanationNode =
  | {
      type: "rule";
      name: string;
      result: string;
      body: string;
      causes?: ExplanationCause[];
      children?: ExplanationNode[];
    }
  | {
      type: "compose";
      expression: string;
      operands: ExplanationNode[];
    }
  | {
      type: "data";
      name: string;
      display: string;
    }
  | {
      type: "data_unused";
      name: string;
    }
  | {
      type: "conversion";
      expression: string;
      steps: ExplanationConversionStep[];
      operands: ExplanationNode[];
    }
  | {
      type: "veto";
      message?: string;
    }
  | {
      type: "unit_equivalence";
      text: string;
    };

/** Root and nested rule explanation (same shape). */
export interface Explanation {
  type: "rule";
  name: string;
  result: string;
  body: string;
  causes?: ExplanationCause[];
  children?: ExplanationNode[];
}

export interface RuleResultPayload {
  measure?: Record<string, string> | null;
  ratio?: Record<string, string> | null;
  number?: string | null;
  boolean?: boolean | null;
  text?: string | null;
  date?: unknown | null;
  time?: unknown | null;
  calendar?: { value: string; unit: string } | null;
}

/** Half-open `[effective_from, effective_to)` for one loaded temporal row. */
export interface ShowVersion {
  effective_from?: string | null;
  effective_to?: string | null;
}

/** Return shape of {@link Engine.show}. */
export interface Show {
  spec: string;
  commentary?: string | null;
  effective_from?: string | null;
  effective_to?: string | null;
  start_line: number;
  source_type?: string | null;
  versions?: ShowVersion[];
  data: Record<string, DataEntry>;
  /** Rule result types; measure and ratio entries expose `units[]` like their data counterparts. */
  rules: Record<string, LemmaType>;
  meta: Record<string, unknown>;
}

/** JSON mirror of slim listed spec row (engine `list`). */
export interface ListedSpecJson {
  name: string;
  effective_from?: DateTimeValueJson | null;
  effective_to?: DateTimeValueJson | null;
}

/** JSON mirror of Rust `ResolvedRepository` (engine `list`). */
export interface ResolvedRepositoryJson {
  repository: string | null;
  specs: ListedSpecJson[];
}

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

/** JSON mirror of Rust `ResourceLimits`. */
export interface ResourceLimitsJson {
  max_source_size_bytes: number;
  max_expression_depth: number;
  max_expression_count: number;
  max_data_value_bytes: number;
  max_loaded_bytes: number;
  max_sources: number;
  max_normalized_expression_nodes: number;
  max_spec_dependency_depth: number;
  max_dag_specs: number;
  max_normal_form_depth: number;
}
