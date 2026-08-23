**Data: spoken question, not precomputed answer**

`data` declares variables. Constraints define validity. Type-only `data` (no value) is an input slot. Use real domain values. Never `"TODO"` or dummy placeholders.

**Spoken question**

Each `data` field's `-> help` is the sentence you would say to a person about their situation. If that sentence requires counting, converting, applying a legal threshold, or interpreting the policy, the field is wrong: those are `rule`s. Dates, amounts, and yes/no facts of the situation stay as `data`.

WRONG (elapsed time as a bare number; unit baked into the name):
```lemma-skip
data days_overdue: number
  -> help "How many days late is the book?"
```

RIGHT (facts of the situation; duration keeps its unit):
```lemma
spec library_return

uses lemma units

data due_date: date
  -> help "When was the book due?"

data return_date: date
  -> help "When was the book returned?"


rule overdue:
  due_date...return_date as day
```

Name the concept; keep the unit on the value, not in the field name (`see **Units**`).

WRONG (policy threshold already applied):
```lemma-skip
data qualifies_for_free_shipping: boolean
  -> help "Is the order €50 or more?"
```

RIGHT:
```lemma
spec shipping_threshold

data money: measure
  -> unit eur: 1.00
  -> decimals 2

data order_total: money
  -> help "What is the order total?"


rule free_shipping:
  order_total >= 50 eur
```

Age-in-years can stay when that is what you would actually ask (e.g. senior discount). A birthday gate wants a birth date.

**Measure**

Use `measure` when the fact carries a unit: money, mass, distance, duration, rates, energy, and other dimensional quantities. Use `number` only when the value is truly dimensionless (counts, scores, IDs). Use `ratio` for proportions (`15%`, `50 permille`), not for currency amounts.

Declare a **named measure parent** once, then extend it on input slots:

```lemma
spec invoice
data money: measure
  -> unit eur: 1.00
  -> decimals 2
  -> minimum 0 eur

data invoice_total: money
  -> help "What is the invoice total?"
```

`-> unit` rows define which units the type accepts; literals and comparisons use those units (`50 eur`, not a bare `50`). Name the concept (`invoice_total`, `package_weight`), not the unit in the field name (`price_eur`, `weight_kg`).

For physical quantities and time, import the standard library (`uses lemma units`) and prefer `units.mass`, `units.duration`, `units.length` over redefining SI units. Duration literals (`8 hour`, `25 year`), conversion (`as <unit>`), compound units (`eur/hour`), measure ranges, and calendar intervals are all covered in **Units**.

**Boolean naming**

Name booleans for the fact you mean; prefer the natural predicate with `-> suggest` for the usual case (`data item_damaged: boolean -> suggest false`). Do not invent awkward opposites (`item_undamaged`) or negated names (`not_*`, `no_*`, `has_no_*`).

**Constraints**

**Types**

Primitives: `boolean`, `number`, `text`, `measure`, `ratio`, `date`, `time`, and `number range`, `measure range`, `date range`, `time range`, `ratio range`. Named parents (`data money: measure`) inherit constraints; see [Reference](https://lemma.run/reference#extending-data).

**Data commands by type**

| Type | `->` commands |
|------|----------------|
| `boolean` | `help`, `suggest` |
| `number` | `decimals`, `minimum`, `maximum`, `help`, `suggest` |
| `measure` | `unit`, `decimals`, `minimum`, `maximum`, `help`, `suggest` (see **Measure**, **Units**) |
| `ratio` | `unit`, `minimum`, `maximum`, `help`, `suggest` |
| `text` | `option`, `options`, `length`, `help`, `suggest` |
| `date`, `time` | `minimum`, `maximum`, `help`, `suggest` |
| `number range`, `date range`, `time range`, `ratio range` | `lower`, `upper`, `minimum`, `maximum`, `help`, `suggest` |
| `measure range` | above + `unit` |

On range types, `lower` / `upper` bound endpoints; `minimum` / `maximum` bound **span width**, not endpoints.

**Usage**

- Chain `->` rows on `data` only (rules have no constraints; see **Syntax**).
- `-> help` = spoken question (CS ask string).
- `-> suggest` = UI hint; does not prefill or commit.
- Prefer `-> option` for closed text sets.
- Bounded domain on constraints (`-> minimum 0`), not veto (see **Veto**).
- Examples: type catalog [01_coffee_order.lemma](https://raw.githubusercontent.com/lemma/lemma/main/engine/documentation/examples/01_coffee_order.lemma); fee policy [02_library_fees.lemma](https://raw.githubusercontent.com/lemma/lemma/main/engine/documentation/examples/02_library_fees.lemma).
- Edge cases (compound units, qualifying units, calendar ranges): **Units** and [Reference](https://lemma.run/reference#data-commands).
