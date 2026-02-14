# Parser Improvements: Typed Constraint Arguments in the AST

## Problem

The Pest grammar already distinguishes four kinds of constraint arguments:

```pest
command_arg = { number_literal | boolean_literal | text_literal | label }
```

But `command_arg_value()` in `types.rs` throws away this distinction and returns a plain `String`. The AST stores constraints as `Option<Vec<(String, Vec<String>)>>` — command name + flat string args. This has three consequences:

1. **Formatter needs a lookup table.** The entire `ArgKind` / `constraint_arg_kinds` / `format_arg_as_source` machinery in `ast.rs` exists solely to reconstruct information the parser already had.
2. **Planning can't distinguish literal kinds.** `[number -> default "10"]` (text literal) and `[number -> default 10]` (number literal) both arrive as the string `"10"`. Planning accepts both because it calls `.parse::<Decimal>()` on the string content — it can't tell the user wrote a quoted text literal where a number was expected.
3. **Named types are a black hole.** For `[custom -> default "single"]` where `custom` is user-defined, `constraint_arg_kinds` returns `None` and the formatter falls back to quoting everything as text.

## Solution

Replace the lossy `Vec<String>` with a typed `CommandArg` enum that preserves what the parser actually matched, so the information flows through the entire pipeline without reconstruction or heuristics.

---

## Step 1: Define `CommandArg` in the AST (`ast.rs`)

Add a new enum that mirrors the four alternatives in the `command_arg` grammar rule:

```rust
/// A parsed constraint command argument, preserving the literal kind
/// from the grammar rule `command_arg = { number_literal | boolean_literal | text_literal | label }`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum CommandArg {
    /// Matched `number_literal` (e.g. `10`, `3.14`)
    Number(String),
    /// Matched `boolean_literal` (e.g. `true`, `false`, `yes`, `no`)
    Boolean(String),
    /// Matched `text_literal` (e.g. `"hello"`) — stores the content between quotes, no surrounding quotes
    Text(String),
    /// Matched `label` (an identifier: `eur`, `kilogram`, `hours`)
    Label(String),
}
```

Add a `.value() -> &str` accessor that returns the inner string regardless of variant, so callers that just need the string content (e.g. `.parse::<Decimal>()`) can use it with minimal disruption.

Add a type alias to reduce signature noise:

```rust
pub type Constraint = (String, Vec<CommandArg>);
```

## Step 2: Update constraint storage in AST types

Replace every occurrence of `Vec<(String, Vec<String>)>` with `Vec<Constraint>`:

- `FactValue::TypeDeclaration { constraints: Option<Vec<Constraint>> }`
- `TypeDef::Regular { constraints: Option<Vec<Constraint>> }`
- `TypeDef::Import { constraints: Option<Vec<Constraint>> }`
- `TypeDef::Inline { constraints: Option<Vec<Constraint>> }`
- `TypeArrowChainResult` type alias in `types.rs`

## Step 3: Update the parser (`types.rs`)

Change `command_arg_value` to return `CommandArg`:

```rust
fn command_arg_value(pair: Pair<Rule>) -> CommandArg {
    let raw = pair.as_str().to_string();
    let mut inner = pair.into_inner();
    let Some(child) = inner.next() else {
        return CommandArg::Label(raw); // label is silent — no child
    };
    match child.as_rule() {
        Rule::text_literal => {
            let s = child.as_str();
            let content = if s.len() >= 2 { s[1..s.len()-1].to_string() } else { s.to_string() };
            CommandArg::Text(content)
        }
        Rule::number_literal => CommandArg::Number(child.as_str().to_string()),
        Rule::boolean_literal => CommandArg::Boolean(child.as_str().to_string()),
        _ => CommandArg::Label(child.as_str().to_string()),
    }
}
```

Update `parse_command` return type from `Vec<String>` to `Vec<CommandArg>`.

## Step 4: Update planning (`semantics.rs`)

Change `apply_constraint` signature from `args: &[String]` to `args: &[CommandArg]`.

Two patterns emerge in the handler code:

### a) Commands that just need the string value

Commands like `help`, `option`, `minimum` for number types — use `arg.value()` where `args.first()` was used before. These still call `.parse::<Decimal>()`, `.parse::<BooleanValue>()`, etc. on the string content. Minimal code change.

### b) Commands where the literal kind matters

Primarily `default` — add an explicit match on the variant to reject wrong literal kinds:

```rust
// Number type, "default" command:
"default" => {
    let arg = args.first().ok_or_else(|| ...)?;
    match arg {
        CommandArg::Number(s) => {
            let d = s.parse::<Decimal>().map_err(|_| ...)?;
            *default = Some(d);
        }
        _ => return Err(format!(
            "default for number type requires a number literal, got {:?}", arg
        )),
    }
}
```

This gives planning **air-tight validation** for both primitive and named types:

- `[number -> default 10]` — parser produces `CommandArg::Number("10")` — accepted.
- `[number -> default "10"]` — parser produces `CommandArg::Text("10")` — rejected: wrong literal kind.
- `[custom -> default "10"]` where `custom = number` — same: `CommandArg::Text("10")` — rejected after type resolution reveals the primitive is `number`.

For named types specifically: planning already resolves the underlying primitive. After resolution, it applies the same constraint logic, so the `CommandArg` variant check works uniformly.

## Step 5: Simplify the formatter (`AsLemmaSource` in `ast.rs`)

The `CommandArg` carries its own formatting knowledge. Add:

```rust
impl fmt::Display for AsLemmaSource<'_, CommandArg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            CommandArg::Text(s) => write!(f, "{}", quote_lemma_text(s)),
            CommandArg::Number(s) | CommandArg::Boolean(s) | CommandArg::Label(s) => {
                write!(f, "{}", s)
            }
        }
    }
}
```

Then **delete** all of the following, which become obsolete:

- `ArgKind` enum
- `constraint_arg_kinds()` function (the entire base-type/command lookup table)
- `format_arg_as_source()` function
- `format_args_with_kinds()` function
- `format_constraint_as_source()` function

`format_constraints_as_source()` simplifies to just iterating over `(cmd, args)` pairs and calling `AsLemmaSource` on each `CommandArg`.

## Step 6: Update all callers

Files that construct or consume `Vec<(String, Vec<String>)>`:

| File | What changes |
|------|-------------|
| `engine/src/parsing/types.rs` | Parser return types, test assertions (e.g. `assert_eq!(constraints[0].1, vec![CommandArg::Number("0".into())])`) |
| `engine/src/planning/semantics.rs` | `apply_constraint` signature and handler bodies |
| `engine/src/formatting/mod.rs` | Constraint formatting delegates to `AsLemmaSource<CommandArg>` |
| `engine/src/parsing/ast.rs` | Remove `ArgKind` machinery, add `CommandArg` + `AsLemmaSource<CommandArg>`, update `Display` impls for `TypeDef` and `AsLemmaSource<FactValue>` |
| `engine/src/planning/validation.rs` | Test constructions if any build constraints manually |
| `engine/src/planning/execution_plan.rs` | Test constructions if any build constraints manually |
| `engine/src/engine.rs` | Planning-level rejection tests can now assert the correct reason (wrong literal kind, not just parse failure) |
| `cli/src/interactive.rs` | If it reads constraint args, use `.value()` |
| `openapi/src/lib.rs` | If it reads constraint args, use `.value()` |

## Step 7: Tests

1. **Parser unit tests** (`types.rs`): Assert that the parser produces the correct `CommandArg` variant for each literal kind.
2. **Round-trip formatter tests** (`ast.rs`/`formatting`): Existing tests should continue to pass since `AsLemmaSource<CommandArg>` produces the same output.
3. **Planning rejection tests** (`engine.rs`): `[number -> default "10"]` is now rejected because `CommandArg::Text("10")` fails the variant check. Same for `[number -> default "10,999.1"]`.
4. **Planning acceptance tests**: `[number -> default 10]` continues to work because `CommandArg::Number("10")` passes.

## Summary of what gets added, changed, and removed

| Added | Changed | Removed |
|-------|---------|---------|
| `CommandArg` enum with 4 variants | `command_arg_value` returns `CommandArg` | `ArgKind` enum |
| `CommandArg::value() -> &str` | All constraint storage: `Vec<String>` → `Vec<CommandArg>` | `constraint_arg_kinds()` |
| `AsLemmaSource<CommandArg>` Display impl | `apply_constraint` validates literal kind for `default` | `format_arg_as_source()` |
| `Constraint` type alias | Formatter uses `AsLemmaSource<CommandArg>` directly | `format_args_with_kinds()` |
| | | `format_constraint_as_source()` |

The net effect: the parser's knowledge is preserved in the AST, flows through to planning for strict validation, and flows through to formatting for correct output — with zero heuristics, zero lookup tables, and zero guessing.
