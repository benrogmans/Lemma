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
     * - object → labeled sources (key insertion order); `[label, code][]` → labeled sources (array order)
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
    list(): ResolvedRepository[];

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
    limits(): ResourceLimits;

    /**
     * Canonical formatting of Lemma source. Throws `EngineError` on parse error.
     * `attribute` is an optional path label used in error messages.
     */
    format(code: string, attribute?: string | null): string;

    /**
     * Evaluate a spec. Pass integers as numbers, decimals as strings in `data`.
     */
    run(options: RunOptions): Response;
  }
}

/** Options for {@link Engine.run}. */
export interface RunOptions {
  /** Spec name (required). */
  spec: string;
  /** Repository qualifier (e.g. `@org/repo`), or omit for workspace. */
  repository?: string | null;
  /** ISO datetime for temporal resolution, or omit for now. */
  effective?: string | null;
  /** Input data values. Pass integers as numbers, decimals as strings. */
  data?: Record<string, unknown>;
  /** Rule names to evaluate, or omit for all rules. */
  rules?: string[] | string | null;
  /** Include explanation tree in response. */
  explain?: boolean;
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
  /** Present only for `kind: "registry"`. */
  registry_kind:
    | "not_found"
    | "unauthorized"
    | "network_error"
    | "server_error"
    | "other"
    | null;
  /** Present only for `kind: "request"`. */
  request_kind: "spec_not_found" | "rule_not_found" | "invalid_request" | null;
  /** Present only for `kind: "resource_limit"`. */
  limit_name: string | null;
  limit_value: string | null;
  actual_value: string | null;
}

// ---------------------------------------------------------------------------
// Show envelope (return shape of Engine.show)
// ---------------------------------------------------------------------------

/**
 * API value fields shared by `RuleResult` (flattened into its top-level fields),
 * `ShowData.prefilled`, `ShowData.suggestion`, and range endpoints.
 * A `None` field is absent (not `null`) per Rust `skip_serializing_if`.
 * When present: always `display`, plus exactly one typed field.
 */
export interface RuleResultValueEndpoint {
  /** Engine-rendered string (`LiteralValue::display_value`). */
  display?: string;
  /** All declared measure units, keyed by unit name. */
  measure?: Record<string, string>;
  /** All declared ratio units, keyed by unit name. */
  ratio?: Record<string, string>;
  number?: string;
  boolean?: boolean;
  text?: string;
  date?: string;
  time?: string;
  calendar?: { value: string; unit: string };
}

/**
 * API value shared by `RuleResult` (flattened into its top-level fields),
 * `ShowData.prefilled`, and `ShowData.suggestion`. When present: always `display`,
 * plus exactly one typed field for a non-range value; `range` is set instead for a
 * range value. A range endpoint (`range.from`/`range.to`) never itself carries a
 * `range` field.
 */
export interface RuleResultValue extends RuleResultValueEndpoint {
  range?: { from: RuleResultValueEndpoint; to: RuleResultValueEndpoint };
}

/** Where a custom type's extension chain is rooted: local to this spec, or imported. */
export type TypeDefiningSpec = { kind: "local" } | { kind: "import" };

export type TypeExtends =
  | { kind: "primitive" }
  | {
      kind: "custom";
      parent: string;
      family: string;
      defining_spec: TypeDefiningSpec;
    };

/** A unit-scoped bound (Measure/DateRange/TimeRange/MeasureRange minimum/maximum/lower/upper). */
export interface NamedBound {
  value: string;
  unit: string;
}

export interface MeasureUnit {
  name: string;
  factor: { numer: string; denom: string };
  /** (measure_ref, exponent) pairs from a compound unit declaration (e.g. meter/second). */
  derived_measure_factors: [string, number][];
  decomposition: Record<string, number>;
  minimum?: string;
  maximum?: string;
  suggestion?: string;
}

export interface RatioUnit {
  name: string;
  value: { numer: string; denom: string };
  minimum?: string;
  maximum?: string;
  suggestion?: string;
}

/**
 * Discriminated union over the 12 Lemma type kinds reachable at the API boundary.
 * Field `kind` is the serde tag; kind-specific fields sit at the top level next to
 * `kind`, `name`, and `extends`. The `veto`/`undetermined` `TypeSpecification`
 * variants are internal sentinels that never reach a successfully planned API
 * response and are intentionally excluded.
 */
export type LemmaType =
  & { name: string | null; extends: TypeExtends }
  & (
    | { kind: "boolean"; help: string }
    | {
        kind: "measure";
        minimum: NamedBound | null;
        maximum: NamedBound | null;
        decimals: number | null;
        units: MeasureUnit[];
        traits: ("duration" | "calendar")[];
        decomposition: Record<string, number> | null;
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
        kind: "numberrange";
        lower: string | null;
        upper: string | null;
        minimum: string | null;
        maximum: string | null;
        help: string;
      }
    | {
        kind: "ratio";
        minimum: string | null;
        maximum: string | null;
        decimals: number | null;
        units: RatioUnit[];
        help: string;
      }
    | {
        kind: "ratiorange";
        lower: string | null;
        upper: string | null;
        minimum: string | null;
        maximum: string | null;
        units: RatioUnit[];
        help: string;
      }
    | {
        kind: "text";
        length: number | null;
        options: string[];
        help: string;
      }
    | { kind: "date"; minimum: string | null; maximum: string | null; help: string }
    | {
        kind: "daterange";
        lower: string | null;
        upper: string | null;
        minimum: NamedBound | null;
        maximum: NamedBound | null;
        help: string;
      }
    | { kind: "time"; minimum: string | null; maximum: string | null; help: string }
    | {
        kind: "timerange";
        lower: string | null;
        upper: string | null;
        minimum: NamedBound | null;
        maximum: NamedBound | null;
        help: string;
      }
    | {
        kind: "measurerange";
        lower: NamedBound | null;
        upper: NamedBound | null;
        minimum: NamedBound | null;
        maximum: NamedBound | null;
        units: MeasureUnit[];
        decomposition: Record<string, number> | null;
        help: string;
      }
  );

/** One input declared in a spec. Omitted fields are absent (not `null`). */
export interface ShowData {
  type: LemmaType;
  /** Spec literal or literal `with` binding; UIs may skip review. */
  prefilled?: RuleResultValue;
  /** `-> suggest ...` suggestion; prompt with prefill in interactive UIs. */
  suggestion?: RuleResultValue;
  /** Local rule names that transitively need this data (planning time). */
  needed_by_rules: string[];
}

/** Return shape of {@link Engine.run}. */
export interface Response {
  spec: string;
  effective: string;
  /** Declared temporal window of the resolved spec version actually evaluated. */
  spec_effective_from?: string;
  spec_effective_to?: string;
  results: Record<string, RuleResult>;
}

/**
 * A rule's result. Fields of `RuleResultValue` are flattened directly onto this
 * object (the Rust side uses `#[serde(flatten)]`), so a measure result's map appears
 * at `result.measure`, not nested under a `value` key.
 */
export type RuleResult = RuleResultValue & {
  vetoed: boolean;
  veto_reason?: string;
  rule_type: string;
  /** Input keys still unbound for this rule (run-data-aware; same keys as Show.data). */
  missing_data?: string[];
  /** Present when `run(..., explain: true)`. Shape: engine/schemas/api.v1.json (`RuleResult.explanation`). */
  explanation?: Explanation;
};

/** One evaluated unless condition, stated as a fact. */
export interface Cause {
  condition: string;
  value: string;
  children?: ExplanationNode[];
}

export interface ConversionStep {
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
      causes?: Cause[];
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
      steps: ConversionStep[];
      operands: ExplanationNode[];
    }
  | {
      type: "veto";
      message?: string;
    };

/** Root and nested rule explanation (same shape). */
export interface Explanation {
  type: "rule";
  name: string;
  result: string;
  body: string;
  causes?: Cause[];
  children?: ExplanationNode[];
}

/** Half-open `[effective_from, effective_to)` for one loaded temporal row. */
export interface ShowVersion {
  effective_from?: string;
  effective_to?: string;
}

/** Provenance of a loaded source. Externally tagged; the unit `Volatile` variant
 *  is the bare string `"volatile"`. */
export type SourceType = "volatile" | { path: string } | { dependency: string };

/** Parsed literal value (meta field value). Externally tagged. */
export type LiteralValue =
  | { number: string }
  | { number_with_unit: [string, string] }
  | { text: string }
  | { date: string }
  | { time: string }
  | { boolean: "true" | "false" | "yes" | "no" }
  | { range: [LiteralValue, LiteralValue] };

/** Spec `meta` field value. Externally tagged. */
export type MetaValue = { literal: LiteralValue } | { unquoted: string };

/** Return shape of {@link Engine.show}. */
export interface Show {
  spec: string;
  commentary?: string;
  effective_from?: string;
  effective_to?: string;
  start_line: number;
  source_type?: SourceType;
  versions?: ShowVersion[];
  data: Record<string, ShowData>;
  /** Rule result types; measure and ratio entries expose `units[]` like their data counterparts. */
  rules: Record<string, LemmaType>;
  meta: Record<string, MetaValue>;
}

/** Slim listed spec row (engine `list`). */
export interface ListedSpec {
  name: string;
  effective_from?: string;
  effective_to?: string;
}

/** Rust `ResolvedRepository` (engine `list`). */
export interface ResolvedRepository {
  /** Absent for the local workspace group (only real repositories carry a name). */
  repository?: string;
  specs: ListedSpec[];
}

/** Rust `ResourceLimits`. */
export interface ResourceLimits {
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
