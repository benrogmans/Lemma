# Relative date expressions

## Problem

Expressing date logic relative to evaluation time currently requires a `today` fact:

```lemma
fact today = [date]
rule delivered_recently = delivered >= today - 7 days and delivered <= today
rule months_employed    = (today - hire_date) in months
```

This forces every document that needs "relative to now" logic to declare and wire up a fact for what is a universal operational concept.

---

## `now`

`now` is a reserved keyword that resolves to the current datetime, injected by the engine at the start of evaluation. It behaves as a `date` value and can be used anywhere a date expression is valid:

```lemma
rule delivered_recently = delivered >= now - 7 days and delivered <= now
rule months_employed    = (now - hire_date) in months
rule contract_active    = contract_end > now
rule days_to_launch     = (launch - now) in days
```

`now` always has `Microsecond` granularity (a full datetime from the system clock).

### Injection

The engine populates `now` from the system clock at the start of each `evaluate` call. API callers can supply a different value for back-testing or auditing:

```rust
pub fn evaluate(
    &self,
    doc_name: &str,
    rule_names: &[&str],
    facts: Facts,
    now: Option<SemanticDateTime>,  // None = system clock
) -> LemmaResult<Response>
```

`EvaluationContext` always carries a fully resolved `now: SemanticDateTime`. It is never absent during evaluation.

---

## Sugar operators

The following forms are syntactic sugar over `now`-based arithmetic. They exist for readability:

### Boolean: rolling window

| Expression | Desugars to |
|-----------|-------------|
| `<date> in past` | `date < now` |
| `<date> in past <duration>` | `date >= now - duration and date <= now` |
| `<date> in future` | `date > now` |
| `<date> in future <duration>` | `date >= now and date <= now + duration` |

`<duration>` may be a literal (`7 days`, `1 year`) or a fact/rule reference of type `duration`.

### Examples

Each form serves a distinct pattern that arises naturally in legal agreements, regulations, and SOPs.

**`now` arithmetic — elapsed and remaining time.** `date - date` returns a duration; use `in <unit>` to convert. Boundary semantics (`<` vs `<=`) are explicit in the rule.

```lemma
doc eligibility

fact birth_date     = [date]
fact hire_date      = [date]
fact incident_date  = [date]
fact purchase_date  = [date]
fact termination_date = [date]
fact filing_deadline  = [date]
fact expiry_date      = [date]
fact notice_period    = [duration]

rule is_adult               = now - birth_date >= 18 years
rule senior_discount        = now - birth_date >= 65 years
rule probation_complete     = now - hire_date >= 6 months
rule warranty_expired       = now - purchase_date > 24 months
rule statute_expired        = now - incident_date >= 3 years
rule months_employed        = (now - hire_date) in months
rule days_to_launch         = (launch - now) in days
rule notice_adequate        = termination_date - now >= 90 days
rule renewal_window_open    = termination_date - now <= 60 days
rule deadline_critical      = filing_deadline - now <= 14 days
rule termination_permitted  = termination_date - now >= notice_period
```

**`in past` / `in future` — window checks.** Common in consumer rights, SLAs, and SOPs where a date either falls within a defined period or it does not. Boundaries are always inclusive.

```lemma
doc consumer_rights

fact delivery_date    = [date]
fact payment_due_date = [date]
fact amendment_date   = [date]
fact notice_period    = [duration]

rule cancellation_eligible = delivery_date in past 30 days
rule grace_period_active   = payment_due_date in future 5 days
rule termination_in_window = termination_date in future notice_period
```

**Calendar operators — fiscal and reporting periods.** Regulations and financial SOPs refer to calendar-aligned periods, not rolling windows.

```lemma
doc reporting

fact invoice_date    = [date]
fact transaction_date = [date]
fact signing_date    = [date]

rule this_year_invoice     = invoice_date in calendar year
rule last_year_transaction = transaction_date in past calendar year
rule signed_this_month     = signing_date in calendar month
rule not_this_year         = invoice_date not in calendar year
```

---

## Calendar operators

Calendar-aligned operators cannot be cleanly expressed with `now`-arithmetic because they reference calendar boundaries (Jan 1, Dec 31, first of month, ISO week Monday) rather than rolling windows. These are first-class operators, not sugar.

| Expression | Meaning |
|-----------|---------|
| `<date> in calendar year` | date falls within the current calendar year |
| `<date> in past calendar year` | date falls within the previous calendar year |
| `<date> in future calendar year` | date falls within the next calendar year |
| `<date> not in calendar year` | date does not fall within the current calendar year |

Supported calendar units: `year`, `month`, `week`.

Calendar boundaries are inclusive (start of first day to end of last day of the period):
- `calendar year`: Jan 1 – Dec 31 of the eval year
- `calendar month`: 1st – last day of the eval month
- `calendar week`: Monday – Sunday of the eval week (ISO 8601)

`in past calendar year` / `in future calendar year` refer to the immediately preceding / following calendar period, not a rolling window.

---

## Date granularity

The grammar is extended to support all ISO 8601 reduced-precision date forms:

| Literal form | Granularity | Internal representation |
|-------------|-------------|------------------------|
| `2026` | `Year` | Jan 1, 00:00:00.000000 |
| `2026-02` | `Month` | 1st of month, 00:00:00.000000 |
| `2026-W08` | `Week` | ISO Monday of that week, 00:00:00.000000 |
| `2026-02-26` | `Day` | 00:00:00.000000 |
| `2026-02-26T14:30` | `Minute` | seconds = 0, microseconds = 0 |
| `2026-02-26T14:30:00` | `Second` | microseconds = 0 |
| `2026-02-26T14:30:00.123` | `Millisecond` | microseconds = 123000 |
| `2026-02-26T14:30:00.123456` | `Microsecond` | as-is |
| `2026-02-26T14:30:00.123456Z` | `Microsecond` | as-is |

Coarser forms fill in missing components with the start of the period. The `granularity` field records what was actually expressed.

### Truncation in sugar operators

When a sugar operator evaluates, `now` is **truncated to the granularity of the date operand** before comparison or subtraction. This ensures the comparison is meaningful at the precision the author expressed:

| Granularity | Truncation applied to `now` |
|-------------|----------------------------|
| `Year` | Jan 1, 00:00:00.000000 of current year |
| `Month` | 1st of current month, 00:00:00.000000 |
| `Week` | ISO Monday of current week, 00:00:00.000000 |
| `Day` | 00:00:00.000000, preserve timezone |
| `Minute` | seconds = 0, microseconds = 0 |
| `Second` | microseconds = 0 |
| `Millisecond` | sub-millisecond digits = 0 |
| `Microsecond` | as-is |

```
2026-02-26 in past
  now       = 2026-02-26T14:30:00Z
  truncated = 2026-02-26T00:00:00Z   [Day]
  2026-02-26 < 2026-02-26 → false   (today is not in the past)

2025 in past
  now       = 2026-02-26T14:30:00Z
  truncated = 2026-01-01T00:00:00Z   [Year]
  2025-01-01 < 2026-01-01 → true

days since 2026-02-26
  now       = 2026-02-26T14:30:00Z
  truncated = 2026-02-26T00:00:00Z   [Day]
  (2026-02-26 - 2026-02-26) in days → 0   (today is 0 days ago, not 0.6)
  Note: when using now directly — (now - 2026-02-26) in days → 0.60 (no truncation)
```

Truncation applies **only** in the sugar operators. When `now` is used directly in expressions, it retains full `Microsecond` precision — the author is responsible for the comparison semantics.

---

## Grammar (`lemma.pest`)

### `now` keyword

```pest
now_literal = { ^"now" }
```

Added to `primary` alongside other literals.

### Date literal grammar (extended for ISO 8601 granularity)

```pest
date_time_literal = {
    date_year_month_day_time  // YYYY-MM-DDThh:mm[:ss][tz]  — tried first (longest)
  | date_year_month_day       // YYYY-MM-DD
  | date_year_week            // YYYY-Www
  | date_year_month           // YYYY-MM
  | date_year_only            // YYYY
}

fractional_seconds = _{ "." ~ ASCII_DIGIT{1,6} }

date_year_month_day_time = {
    year ~ "-" ~ month ~ "-" ~ day
    ~ "T" ~ hour ~ ":" ~ minute ~ (":" ~ second ~ fractional_seconds?)? ~ timezone?
}
date_year_month_day = { year ~ "-" ~ month ~ "-" ~ day }
date_year_week      = { year ~ "-W" ~ week_number }
date_year_month     = { year ~ "-" ~ month }
date_year_only      = { year }

week_number = _{ ASCII_DIGIT{2} }
```

`date_year_only` is only attempted in contexts where a date literal is expected to avoid matching bare numbers in arithmetic.

### Sugar operator grammar

```pest
date_relative_expression = {
    base_expression ~ SPACE+ ~ ^"in" ~ SPACE+ ~ (
        (^"past"   ~ (SPACE+ ~ simple_expression)?)
      | (^"future" ~ (SPACE+ ~ simple_expression)?)
    )
}

calendar_unit = { ^"year" | ^"month" | ^"week" }

date_calendar_expression = {
    base_expression ~ SPACE+ ~ (
        (^"in" ~ SPACE+ ~ ^"past"   ~ SPACE+ ~ ^"calendar" ~ SPACE+ ~ calendar_unit)
      | (^"in" ~ SPACE+ ~ ^"future" ~ SPACE+ ~ ^"calendar" ~ SPACE+ ~ calendar_unit)
      | (^"in" ~ SPACE+             ~          ^"calendar" ~ SPACE+ ~ calendar_unit)
      | (^"not" ~ SPACE+ ~ ^"in"   ~ SPACE+   ^"calendar" ~ SPACE+ ~ calendar_unit)
    )
}

comparison_or_relative = {
    date_calendar_expression
  | date_relative_expression
  | comparison_expression
  | conversion_expression
  | base_expression
}
```

---

## AST (`parsing/ast.rs`)

```rust
pub enum ExpressionKind {
    // … existing variants …
    Now,
    DateRelative(DateRelativeKind, Box<Expression>, Option<Box<Expression>>),
    DateCalendar(DateCalendarKind, CalendarUnit, Box<Expression>),
}

pub enum DateRelativeKind {
    InPast,    // optional duration
    InFuture,  // optional duration
}

pub enum DateCalendarKind {
    Current,   // in calendar <unit>
    Past,      // in past calendar <unit>
    Future,    // in future calendar <unit>
    NotIn,     // not in calendar <unit>
}

pub enum CalendarUnit { Year, Month, Week }

pub enum DateGranularity { Year, Month, Week, Day, Minute, Second, Millisecond, Microsecond }
```

`SemanticDateTime` gains `microsecond: u32` (0–999999) and `granularity: DateGranularity`, both set at parse time. `now` from the system clock always has `Microsecond` granularity.

---

## Evaluation semantics

### `now`

Reads `context.now` directly. Type: `date`. Granularity: `Microsecond`.

### Sugar operators (truncate `now` to operand granularity first)

```
in past (no D)   →  date < now_t
in past D        →  date >= (now_t - D) and date <= now_t
in future (no D) →  date > now_t
in future D      →  date >= now_t and date <= (now_t + D)
```

where `now_t = truncate(context.now, date.granularity)`.

### Calendar-aligned

```
in calendar year        →  Jan 1 now.year  <= date <= Dec 31 now.year
in past calendar year   →  Jan 1 (year-1)  <= date <= Dec 31 (year-1)
in future calendar year →  Jan 1 (year+1)  <= date <= Dec 31 (year+1)
not in calendar year    →  date < Jan 1 now.year  or  date > Dec 31 now.year
```

`month` and `week` follow the same pattern with their respective boundaries.

Implementation helpers in `computation/datetime.rs`:
- `truncate_to_granularity(now, granularity) → SemanticDateTime`
- `compute_date_relative_range(kind, date, Option<duration>, now) → OperationResult`
- `compute_date_calendar(kind, unit, date, now) → OperationResult`

---

## Formatter

| AST | Emitted Lemma |
|-----|--------------|
| `Now` | `now` |
| `DateRelative(InPast, expr, None)` | `<expr> in past` |
| `DateRelative(InPast, expr, Some(dur))` | `<expr> in past <dur>` |
| `DateRelative(InFuture, expr, None)` | `<expr> in future` |
| `DateRelative(InFuture, expr, Some(dur))` | `<expr> in future <dur>` |
| `DateCalendar(Current, unit, expr)` | `<expr> in calendar <unit>` |
| `DateCalendar(Past, unit, expr)` | `<expr> in past calendar <unit>` |
| `DateCalendar(Future, unit, expr)` | `<expr> in future calendar <unit>` |
| `DateCalendar(NotIn, unit, expr)` | `<expr> not in calendar <unit>` |

---

## Proof tree examples

```
delivered in past 7 days
  ├─ now        = 2026-02-26T14:30:00Z   [injected]
  ├─ now_t      = 2026-02-26T00:00:00Z   [truncated to Day]
  ├─ lower      = 2026-02-19T00:00:00Z   [now_t - 7 days]
  ├─ delivered  = 2026-02-22             [fact]
  └─ 2026-02-22 >= 2026-02-19 and 2026-02-22 <= 2026-02-26 → true

invoice_date in past calendar year
  ├─ now          = 2026-02-26T14:30:00Z   [injected]
  ├─ period_start = 2025-01-01
  ├─ period_end   = 2025-12-31
  ├─ invoice_date = 2025-06-15             [fact]
  └─ 2025-06-15 >= 2025-01-01 and 2025-06-15 <= 2025-12-31 → true
```

---

## Test cases

### Unit (`computation/datetime.rs`)

**`now` keyword:**
- `now` resolves to injected datetime; type is `date`; usable in arithmetic and comparisons

**Rolling window — day granularity:**
- `in past`: date is today → false; date is yesterday → true
- `in past 7 days`: date is today → true; date is today-7d → true; date is today-8d → false
- `in future`: date is today → false; date is tomorrow → true

**Rolling window — coarser ISO granularities:**
- `2026 in past` with now in 2026 → false; with now in 2027 → true
- `2026-02 in past` with now in 2026-02 → false; with now in 2026-03 → true
- `2026-W08 in past` with now in week 9 → true; with now in week 8 → false

**Calendar:**
- `in calendar year`: date in current year → true; date in last year → false
- `in past calendar year`: date in last year → true; date in current year → false
- `in calendar month`: date in current month → true; adjacent month → false
- `in calendar week`: ISO Monday of current week → true; day before → false

### Planning

- `now` resolves to type `date` during planning
- Date operand not `date` type → `LemmaError::Engine`
- Duration operand not `duration` type → `LemmaError::Engine`

### Integration (`engine/tests/`)

```lemma
doc relative_dates

fact delivered     = [date]
fact hire_date     = [date]
fact launch        = [date]
fact invoice_date  = [date]
fact notice_period = [duration]

rule days_to_launch      = (launch - now) in days
rule months_employed     = (now - hire_date) in months
rule delivered_recently  = delivered in past 7 days
rule contract_active     = launch in future
rule renewal_imminent    = launch in future notice_period
rule this_year_invoice   = invoice_date in calendar year
rule last_year_invoice   = invoice_date in past calendar year
rule raw_now_comparison  = hire_date <= now
```

- `now = 2026-02-26T00:00:00Z`, `delivered = 2026-02-22` → `delivered_recently = true`
- `now = 2026-02-26T00:00:00Z`, `delivered = 2026-02-18` → `delivered_recently = false`
- `now = 2026-02-26T00:00:00Z`, `hire_date = 2024-08-01` → `months_employed ≈ 18.83`
- `now = 2026-02-26T00:00:00Z`, `launch = 2026-03-01` → `days_to_launch ≈ 3.0`
- `now = 2026-02-26T00:00:00Z`, `invoice_date = 2025-11-15` → `last_year_invoice = true`
- Round-trip: parse → plan → format → re-parse → plan succeeds

---

## Summary

| Layer | Change |
|-------|--------|
| Grammar | `now_literal`; extended `date_time_literal` (ISO 8601 granularities); `date_relative_expression`, `date_calendar_expression` |
| AST | `ExpressionKind::Now`; `DateRelative`, `DateCalendar`; `DateGranularity` on `SemanticDateTime` |
| Planning | `Now` resolves to type `date`; type-check all operator operands |
| `EvaluationContext` | Add `now: SemanticDateTime` (always present) |
| Engine API | `evaluate(…, now: Option<SemanticDateTime>)` — `None` uses system clock |
| Evaluation | `Now` reads `context.now`; sugar operators truncate `now` to operand granularity |
| `computation/datetime.rs` | `truncate_to_granularity` + two operator helpers |
| Formatter | Round-trip all forms including `now` |
