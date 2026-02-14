# Formatter: type-aware constraint args (all commands)

## Problem

Constraint arguments in the AST are stored as `Vec<String>`. The formatter currently emits `args.join(" ")`, which produces invalid Lemma when an argument was originally a **text literal** (e.g. `-> default "single"` becomes `default single`). Each constraint command has a fixed meaning **per type**, so we know exactly what each argument is (number, text, boolean, label, date, time, duration unit). There must be **no defaults and no fallbacks**: every (base type, command) has an explicit formatting rule.

## Lemma argument kinds (emit rules)

When emitting constraint args, use the kind to choose syntax:

| Kind | Lemma syntax | Emit |
|------|--------------|------|
| **number** | number_literal | as-is |
| **boolean** | boolean_literal (true/false/yes/no/accept/reject) | as-is |
| **text** | text_literal | quoted; escape `\` and `"` (same as `format_value(Value::Text(...))`) |
| **label** | identifier (unit name, option name, etc.) | as-is |
| **date** | date or datetime literal (YYYY-MM-DD or with time) | as-is |
| **time** | time literal (HH:MM:SS) | as-is |
| **duration_unit** | label (years, months, hours, …) | as-is |

---

## Explicit (base, command) → arg kinds

Source: `engine/src/planning/semantics.rs` `apply_constraint`, and `documentation/reference.md`. Every command for every type is listed. Formatter must implement exactly these; no other behaviour.

### base = **boolean**

| Command | Args | Arg 0 | Arg 1 |
|---------|------|-------|-------|
| help | 1 | text | — |
| default | 1 | boolean | — |

### base = **scale**

| Command | Args | Arg 0 | Arg 1 |
|---------|------|-------|-------|
| decimals | 1 | number | — |
| unit | 2 | label | number |
| minimum | 1 or 2 | number | label (optional; e.g. `0 eur`) |
| maximum | 1 or 2 | number | label (optional) |
| precision | 1 | number | — |
| help | 1 | text | — |
| default | 2 | number | label |

### base = **number**

| Command | Args | Arg 0 | Arg 1 |
|---------|------|-------|-------|
| decimals | 1 | number | — |
| minimum | 1 | number | — |
| maximum | 1 | number | — |
| precision | 1 | number | — |
| help | 1 | text | — |
| default | 1 | number | — |

### base = **ratio** (and **percent**; same commands)

| Command | Args | Arg 0 | Arg 1 |
|---------|------|-------|-------|
| decimals | 1 | number | — |
| unit | 2 | label | number |
| minimum | 1 | number | — |
| maximum | 1 | number | — |
| help | 1 | text | — |
| default | 1 | number | — |

### base = **text**

| Command | Args | Arg 0..N |
|---------|------|----------|
| option | 1 | text |
| options | N | text, text, … (each quoted) |
| minimum | 1 | number |
| maximum | 1 | number |
| length | 1 | number |
| help | 1 | text |
| default | 1 | text |

### base = **date**

| Command | Args | Arg 0 |
|---------|------|--------|
| minimum | 1 | date |
| maximum | 1 | date |
| help | 1 | text |
| default | 1 | date |

### base = **time**

| Command | Args | Arg 0 |
|---------|------|--------|
| minimum | 1 | time |
| maximum | 1 | time |
| help | 1 | text |
| default | 1 | time |

### base = **duration**

| Command | Args | Arg 0 | Arg 1 |
|---------|------|-------|-------|
| help | 1 | text | — |
| default | 2 | number | duration_unit (label) |

---

## Named base (e.g. `filing_status_type`, `money`, `coffee`)

**No default and no fallback.** One of the following must be chosen explicitly:

- **Option A — Require type resolution:** The formatter (or its caller) receives resolved type information (e.g. base name → primitive or full type spec). When formatting constraints for a fact with a named base, use the resolved type to look up the same (base, command) table (e.g. resolved `filing_status_type` → text ⇒ use **text** row).
- **Option B — Refuse to format:** When `base` is not one of the primitives above (`boolean`, `scale`, `number`, `ratio`, `percent`, `text`, `date`, `time`, `duration`), do not format constraint arguments (e.g. return an error, or emit a placeholder that forces the user to fix manually). No silent “guess”.
- **Option C — Explicit passthrough:** When `base` is not in the primitive set, emit constraint args with a single explicit rule (e.g. “emit `args.join(" ")` and document that round-trip for named-type constraints is not guaranteed”). This is one explicit behaviour, not a fallback.

Implementation must pick A, B, or C and document it. No “if unknown then quote” or “if unknown then as-is” without it being the chosen explicit rule.

---

## Implementation

- In `format_fact_value` (or shared helper), for each `(cmd, args)` in the type declaration:
  - Resolve **base** to a key (primitive name or, if Option A, resolved type).
  - Look up the row for (base, cmd) in the table above.
  - For each arg index, use the stated kind to emit: number/boolean/label/date/time/duration_unit → as-is; text → quoted with escape.
- Reuse existing escaping for text (same as `format_value(Value::Text(...))`).
- No new API surface for the “formatter only” path unless Option A is chosen (then formatter may take an optional type-resolver or pre-resolved map).

---

## Test cases

1. **`[number -> default "10"]`**  
   Args `["10"]`, base `number`, command `default` → arg0 is number → emit `default 10`. Round-trip parses and plans; value is number 10.

2. **`[number -> default "10 $$"]`**  
   Args `["10 $$"]`, base `number`, command `default` → arg0 is number → emit `default 10 $$`. Planning must fail (invalid default). Test: parse → format → assert output contains `default 10 $$`; optionally assert plan fails.

3. **All commands covered**  
   Add tests (or one parameterised test) that for each (base, command) in the table we emit the expected syntax (e.g. `-> option "x"` for text option; `-> unit eur 1.00` for scale unit; `-> help "Description"`; etc.). No behaviour left untested.

4. **Named base**  
   One test that encodes the chosen behaviour for named base (A, B, or C): e.g. if B, assert format errors on `[my_type -> default "x"]`; if A, assert format uses resolved type; if C, assert we emit args with the explicit passthrough rule.

---

## Summary

| Item | Action |
|------|--------|
| **Table** | Every (base, command) has an explicit row; every arg has an explicit kind (number, boolean, text, label, date, time, duration_unit). |
| **Emit** | number/boolean/label/date/time/duration_unit → as-is; text → quoted and escaped. |
| **Named base** | Choose A (resolution), B (refuse), or C (explicit passthrough); document and test. No silent default. |
| **Tests** | `[number -> default "10"]`, `[number -> default "10 $$"]`, coverage for all commands, and named-base behaviour. |
