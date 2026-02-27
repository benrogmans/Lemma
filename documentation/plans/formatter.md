# Lemma Code Formatter

A `lemma fmt` command that produces **canonical, deterministic formatting** of `.lemma` files.
Given any syntactically valid Lemma file, it outputs a consistently formatted version.
Running the formatter twice produces identical output (idempotent).

## Relationship to existing code

- **This plan** describes the **Lemma source code formatter**: formatting `.lemma` file contents (AST → canonical Lemma text). It adds `engine/src/formatting/`, fixes Display in `engine/src/parsing/ast.rs`, and adds the `lemma fmt` CLI subcommand.
- **`cli/src/formatter.rs`** is the **response formatter**: it formats evaluation output for the terminal (e.g. `format_response`, `format_document_inspection`, `format_workspace_summary`, proof trees). It is **not** modified by this plan. The two are separate: one formats Lemma source code; the other formats run/show/list output.

## Approach: AST-based pretty-printing

Parse source code into the AST, then emit formatted output via the existing `Display` trait impls.

This works well for Lemma because:
- Lemma has **no inline comments** — the only documentation syntax (commentary blocks) is captured in the AST
- The AST already captures all semantically meaningful information
- Deterministic output is guaranteed since we generate from a structured representation
- The formatter only requires **parsing**, not planning/validation — so it can format files with semantic errors

## Strategy: Fix the existing `Display` impls

Rather than creating a parallel formatting system, we **fix and extend the existing `Display` implementations**
on the AST types in `engine/src/parsing/ast.rs`. This keeps a single source of truth for "AST → Lemma source."

The formatter module (`engine/src/formatting/`) provides the top-level `format_source()` function
that parses input and calls `Display` on the resulting AST.

### Current Display bugs to fix

| Type | Bug | Fix |
|------|-----|-----|
| `Value::Duration` | Uses `{:?}` (Debug) for unit → `2 Day` | Use `{}` (Display) → `2 days` |
| `Value::Ratio` | Outputs stored ratio `0.10 percent` | Convert back to user form: `10%` |
| `DateTimeValue` | Always outputs `T00:00:00` for date-only | Detect date-only → `2024-01-15` |
| `Expression` | No precedence-aware parenthesization | Add parens based on operator precedence |
| `LemmaRule` | Unless clauses on same line | Each `unless` on its own line, 2-space indent |
| `LemmaDoc` | Missing types section, no blank lines | Add types, proper sections, blank lines |
| `TypeDef` | No Display impl at all | Add Display impl |

### Display impls that are already correct (no changes needed)

- `FactReference` — correct dot-path output
- `RuleReference` — correct `name?` and `path.name?` output
- `DocRef` — correct `@` handling
- `ArithmeticComputation` — correct operator symbols
- `ComparisonComputation` — correct operator symbols
- `MathematicalComputation` — correct function names
- `ConversionTarget` — correct unit output
- `BooleanValue` — correct lowercase via strum
- `TimeValue` — correct `HH:MM:SS` format
- `TimezoneValue` — correct `Z` / `+HH:MM` format
- `Value::Number`, `Value::Text`, `Value::Scale`, `Value::Boolean`, `Value::Time` — all correct

## Canonical Formatting Rules

Derived from analyzing all 17 example `.lemma` files.

### Document structure

```
doc name
"""
Commentary text (if present).
"""

<types section>

<facts section>

<rules section>
```

- `doc name` on its own line
- Commentary block in triple quotes on subsequent lines
- Blank line after doc declaration (or after commentary if present)
- Blank line between sections (types → facts → rules)
- Two blank lines between documents in the same file
- Single trailing newline at end of file

### Type definitions

Standalone type definitions use **multi-line arrow chains** (2-space indent):

```
type money = scale
  -> unit eur 1.00
  -> unit usd 1.10
  -> decimals 2
  -> minimum 0 eur

type status = text
  -> option "active"
  -> option "inactive"
```

Type imports stay on one line (with optional inline arrows):

```
type currency from base_doc
type discount_rate from pricing -> maximum 0.5
```

Blank line between type definitions.

### Arrow chain formatting rule

Arrow chains (`->`) appear in three contexts. The formatting rule:

- **Standalone type definitions** (`type X = Y`): each `->` on its own line, indented 2 spaces
- **Type imports** (`type X from doc`): arrows stay inline on the same line
- **Inline type annotations** (`[Y -> ...]`): arrows stay inline within the brackets

This is a single consistent principle: **block definitions get multi-line arrows;
annotations and imports get inline arrows.**

### Facts

```
fact name = "Alice"
fact age = 32
fact price = [money]
fact pending = [number -> minimum 0 -> maximum 100]
fact employee = doc base_employee
fact employee.name = "Alice Smith"
```

- `fact reference = value` on a single line
- No alignment of `=` signs (canonical form is unaligned for diff-friendliness)

### Rules

```
rule total = subtotal? - discount?

rule discount = 0%
  unless quantity >= 10 then 10%
  unless quantity >= 50 then 20%

rule tax = (bracket_2_limit? - bracket_1_limit?) * tax_bracket_2?
  unless taxable_income? < bracket_2_limit?
    then (taxable_income? - bracket_1_limit?) * tax_bracket_2?
```

- `rule name = expression` on one line (or wrapped at arithmetic operators when over max_cols)
- Each `unless condition then result` on its own line, indented 2 spaces; when the full line would exceed max_cols, the `then` clause goes on the next line with two extra spaces (4 spaces total)
- Blank line between rule definitions

### Soft line length (max_cols = 100)

- **Unless clauses**: If `  unless {condition} then {result}` is longer than 100 characters, the `then` clause is placed on the next line with 4 spaces indentation (e.g. `    then 5%`).
- **Expressions**: If a rule’s expression exceeds 100 characters, it is wrapped at **arithmetic** operators (+, -, *, /, %, ^) only; comparison and logical operators are not split. Continuation lines are indented 2 spaces. Long fact values are not broken (max_cols is soft).

### Expressions

- Spaces around all binary operators: `a + b`, `x >= 10`, `a and b`
- Space after keyword operators: `not x`, `sqrt x`, `value in hours`
- Parentheses added only where necessary for operator precedence correctness
- Parentheses omitted where operator precedence already provides correct grouping

### Blank lines

- One blank line between type definitions (multi-line ones)
- One blank line between the types / facts / rules sections
- One blank line between individual rules
- Two blank lines between documents in the same file
- No trailing blank lines at end of file
- Single trailing newline at end of file

## Architecture

### Formatting module: `engine/src/formatting/mod.rs`

A thin module that provides the public API. The actual formatting work
is done by the `Display` impls on the AST types.

```rust
/// Format a sequence of parsed documents into canonical Lemma source.
pub fn format_docs(docs: &[LemmaDoc]) -> String;

/// Parse a source string and format it. Returns Err if the source doesn't parse.
pub fn format_source(source: &str, attribute: &str) -> Result<String, Error>;
```

Wired into `engine/src/lib.rs`:

```rust
pub mod formatting;
pub use formatting::{format_docs, format_source};
```

### Display impl changes in `engine/src/parsing/ast.rs`

All formatting logic lives in the `Display` impls. The changes:

#### `Value::Display` — fix Duration and Ratio

```rust
Value::Duration(n, u) => write!(f, "{} {}", n, u),  // was {:?}
Value::Ratio(n, u) => {
    match u.as_deref() {
        Some("percent") => {
            // Stored as ratio (0.10), display as percentage (10%)
            let display_value = *n * Decimal::from(100);
            let norm = display_value.normalize();
            let s = if norm.fract().is_zero() {
                norm.trunc().to_string()
            } else {
                norm.to_string()
            };
            write!(f, "{}%", s)
        }
        Some(unit) => { /* existing logic with unit name */ }
        None => { /* existing logic without unit */ }
    }
}
```

#### `DateTimeValue::Display` — detect date-only

```rust
fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let is_date_only = self.hour == 0 && self.minute == 0
                    && self.second == 0 && self.timezone.is_none();
    if is_date_only {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    } else {
        write!(f, "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
               self.year, self.month, self.day,
               self.hour, self.minute, self.second)?;
        if let Some(tz) = &self.timezone {
            write!(f, "{}", tz)?;
        }
        Ok(())
    }
}
```

#### `Expression::Display` — precedence-aware parenthesization

Add a helper function that determines the precedence level of each expression kind:

```rust
fn precedence(kind: &ExpressionKind) -> u8 {
    match kind {
        ExpressionKind::LogicalOr(..)                => 1,
        ExpressionKind::LogicalAnd(..)               => 2,
        ExpressionKind::LogicalNegation(..)          => 3,
        ExpressionKind::Comparison(..)               => 4,
        ExpressionKind::UnitConversion(..)           => 4,
        ExpressionKind::Arithmetic(_, op, _) => match op {
            Add | Subtract    => 5,
            Multiply | Divide | Modulo => 6,
            Power             => 7,
        },
        ExpressionKind::MathematicalComputation(..)  => 8,
        // Primary — never needs parens
        ExpressionKind::Literal(..)
        | ExpressionKind::FactReference(..)
        | ExpressionKind::RuleReference(..)
        | ExpressionKind::UnresolvedUnitLiteral(..)
        | ExpressionKind::Veto(..)                   => 10,
    }
}
```

The Display impl wraps a child expression in parentheses when
`child_precedence < parent_precedence`:

```rust
fn write_child(f: &mut fmt::Formatter<'_>, child: &Expression, parent_prec: u8) -> fmt::Result {
    let child_prec = precedence(&child.kind);
    if child_prec < parent_prec {
        write!(f, "({})", child)
    } else {
        write!(f, "{}", child)
    }
}
```

Each binary expression arm uses `write_child` for its operands:

```rust
ExpressionKind::Arithmetic(left, op, right) => {
    let my_prec = precedence(&self.kind);
    write_child(f, left, my_prec)?;
    write!(f, " {} ", op)?;
    write_child(f, right, my_prec)
}
```

#### `LemmaRule::Display` — unless on separate lines

```rust
fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "rule {} = {}", self.name, self.expression)?;
    for unless in &self.unless_clauses {
        write!(f, "\n  unless {} then {}", unless.condition, unless.result)?;
    }
    writeln!(f)
}
```

#### `LemmaDoc::Display` — full document with types and sections

```rust
fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "doc {}", self.name)?;
    writeln!(f)?;

    if let Some(ref commentary) = self.commentary {
        writeln!(f, "\"\"\"")?;
        writeln!(f, "{}", commentary)?;
        writeln!(f, "\"\"\"")?;
    }

    // Types section (skip Inline — those are part of facts)
    let named_types: Vec<_> = self.types.iter()
        .filter(|t| !matches!(t, TypeDef::Inline { .. }))
        .collect();
    if !named_types.is_empty() {
        writeln!(f)?;
        for (i, td) in named_types.iter().enumerate() {
            if i > 0 { writeln!(f)?; }
            write!(f, "{}", td)?;
            writeln!(f)?;
        }
    }

    // Facts section
    if !self.facts.is_empty() {
        writeln!(f)?;
        for fact in &self.facts {
            write!(f, "{}", fact)?;
        }
    }

    // Rules section
    if !self.rules.is_empty() {
        writeln!(f)?;
        for (i, rule) in self.rules.iter().enumerate() {
            if i > 0 { writeln!(f)?; }
            write!(f, "{}", rule)?;
        }
    }

    Ok(())
}
```

#### New: `TypeDef::Display`

```rust
impl fmt::Display for TypeDef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeDef::Regular { name, parent, constraints, .. } => {
                write!(f, "type {} = {}", name, parent)?;
                if let Some(cs) = constraints {
                    for (cmd, args) in cs {
                        write!(f, "\n  -> {}", cmd)?;
                        for arg in args {
                            write!(f, " {}", arg)?;
                        }
                    }
                }
                Ok(())
            }
            TypeDef::Import { name, from, constraints, .. } => {
                write!(f, "type {} from {}", name, from)?;
                if let Some(cs) = constraints {
                    for (cmd, args) in cs {
                        write!(f, " -> {}", cmd)?;
                        for arg in args {
                            write!(f, " {}", arg)?;
                        }
                    }
                }
                Ok(())
            }
            TypeDef::Inline { .. } => {
                // Inline types are rendered as part of FactValue::TypeDeclaration
                Ok(())
            }
        }
    }
}
```

### CLI command: `lemma fmt`

New subcommand in `cli/src/main.rs`:

```rust
/// Format .lemma files to canonical style
Fmt {
    /// Files or directories to format (default: current directory)
    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,
    /// Check formatting without modifying (exit 1 if unformatted)
    #[arg(long)]
    check: bool,
    /// Write to stdout instead of modifying files
    #[arg(long)]
    stdout: bool,
}
```

Implementation: walk `.lemma` files, call `lemma::format_source()`, compare, write/check.

## Implementation Order

| Step | What | Where |
|------|------|-------|
| 1 | Fix `Value::Display` (Duration, Ratio) | `ast.rs` |
| 2 | Fix `DateTimeValue::Display` (date-only detection) | `ast.rs` |
| 3 | Add `TypeDef::Display` | `ast.rs` |
| 4 | Fix `Expression::Display` (precedence + parens) | `ast.rs` |
| 5 | Fix `LemmaRule::Display` (unless on own lines) | `ast.rs` |
| 6 | Fix `LemmaDoc::Display` (types section, blank lines) | `ast.rs` |
| 7 | Create `engine/src/formatting/mod.rs` (`format_source`, `format_docs`) | new file |
| 8 | Wire into `engine/src/lib.rs` | `lib.rs` |
| 9 | Add `lemma fmt` CLI command | `cli/src/main.rs` |
| 10 | Round-trip and idempotency tests | `engine/tests/`, `ast.rs` |

Steps 1-3 can be done in parallel (no dependencies).
Step 4 depends on step 1 (Value Display must be correct for Expression to produce correct output).
Steps 5-6 depend on step 4.
Steps 7-8 depend on step 6.
Step 9 depends on steps 7-8.
Step 10 validates everything.

## Testing Strategy

### Unit tests (inside `ast.rs` test module)

- **Value::Display**: every variant formats correctly
  - `Ratio(0.10, Some("percent"))` → `"10%"`
  - `Duration(2, Day)` → `"2 days"`
  - `Date(2024,1,15,0,0,0,None)` → `"2024-01-15"`
  - `Date(2024,10,1,17,0,0,Some(Z))` → `"2024-10-01T17:00:00Z"`
- **Expression::Display**: all precedence boundaries
  - `(a + b) * c` keeps parens, `a + b * c` omits them
  - `not a and b` correct grouping
  - `a or b and c` correct grouping
  - math functions: `sqrt x + y` vs `sqrt (x + y)`
- **TypeDef::Display**: Regular with arrows, Import with/without arrows
- **LemmaRule::Display**: unless clauses on separate lines
- **LemmaDoc::Display**: types + facts + rules sections with correct blank lines

### Round-trip integration tests (in `engine/tests/`)

For each of the 17 example `.lemma` files:
1. Parse source → `Vec<LemmaDoc>`
2. Format via Display → `String`
3. Parse formatted string → `Vec<LemmaDoc>`
4. Assert the two ASTs are structurally equal

### Idempotency tests

For each example file:
1. Format once → `output1`
2. Format `output1` → `output2`
3. Assert `output1 == output2`

## Edge Cases

| Case | Handling |
|------|----------|
| Empty document (no types/facts/rules) | `doc name\n` |
| Document with only commentary | `doc name\n"""\n...\n"""\n` |
| Number with thousand separators (`1_000_000`) | Parser strips them; formatter outputs clean `1000000` |
| Scientific notation (`1.23e5`) | Parser converts to `Decimal`; outputs normalized form |
| Deeply nested expressions | Precedence engine handles arbitrary depth |
| Registry references (`@org/...`) | `DocRef.is_registry` flag preserved |
| Multi-doc files | Two blank lines between documents |
| Trailing whitespace / blank lines | Removed; single trailing newline |

## Non-goals for v1

- Configurable formatting options (indent size, alignment, max_cols)
- Partial formatting (format a selection within a file)
- Preserving user-placed blank lines beyond canonical rules
