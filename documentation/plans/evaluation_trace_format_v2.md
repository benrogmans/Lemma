# Evaluation Trace Format v2 (Tree-based)

## Simple Calculation

```
┌────────────┬───────┐
│ Fact       │ Value │
├────────────┼───────┤
│ income     │ 85000 │
│ deductions │ 12000 │
│ state      │ "CA"  │
└────────────┴───────┘

┌─────────────────────────┐
│ taxable_income = 73000  │
├─────────────────────────┤
│ income - deductions     │
│ ├─ = 85000 - 12000      │
│ └─ = 73000              │
└─────────────────────────┘
```

## With Referenced Rules

```
┌───────────────────────────────────────────┐
│ tax_on_bracket_1 = 1100                   │
├───────────────────────────────────────────┤
│ bracket_1_limit? × federal_tax_bracket_1? │
│ ├─ bracket_1_limit?                       │
│ │  └─ = 11000                             │
│ ├─ federal_tax_bracket_1?                 │
│ │  └─ = 10%                               │
│ ├─ = 11000 × 10%                          │
│ └─ = 1100                                 │
└───────────────────────────────────────────┘
```

## Unless Clauses

```
rule tax_on_bracket_3 = (bracket_3_limit? - bracket_2_limit?) * federal_tax_bracket_3?
  unless taxable_income? < bracket_3_limit? then (taxable_income? - bracket_2_limit?) * federal_tax_bracket_3?
  unless taxable_income? < bracket_2_limit? then 0

┌─────────────────────────────────────────────────────────────────┐
│ tax_on_bracket_3 = 6220.50                                      │
├─────────────────────────────────────────────────────────────────┤
│ taxable_income? < bracket_3_limit?                              │
│ ├─ taxable_income?                                              │
│ │  ├─ income - deductions                                       │
│ │  ├─ = 85000 - 12000                                           │
│ │  └─ = 73000                                                   │
│ ├─ = 73000 < 95375                                              │
│ ├─ (taxable_income? - bracket_2_limit?) × federal_tax_bracket_3?│
│ ├─ = (73000 - 44725) × 22%                                      │
│ └─ = 6220.50                                                    │
└─────────────────────────────────────────────────────────────────┘

┌────────────────────────────────────────────────┐
│ tax_on_bracket_4 = 0                           │
├────────────────────────────────────────────────┤
│ taxable_income? <= bracket_3_limit?            │
│ ├─ = 73000 <= 95375                            │
│ └─ = 0                                         │
└────────────────────────────────────────────────┘
```

## Complex Multi-Rule Calculation

```
┌─────────────────────────────────────────────────────────────────┐
│ total_federal_tax = 11367.50                                    │
├─────────────────────────────────────────────────────────────────┤
│ tax_on_bracket_1? + tax_on_bracket_2? + tax_on_bracket_3? + tax_on_bracket_4?
│ ├─ tax_on_bracket_1?                                            │
│ │  ├─ bracket_1_limit? × federal_tax_bracket_1?                 │
│ │  ├─ = 11000 × 10%                                             │
│ │  └─ = 1100                                                    │
│ ├─ tax_on_bracket_2?                                            │
│ │  ├─ (bracket_2_limit? - bracket_1_limit?) × federal_tax_bracket_2?
│ │  ├─ = (44725 - 11000) × 12%                                   │
│ │  ├─ = 33725 × 12%                                             │
│ │  └─ = 4047                                                    │
│ ├─ tax_on_bracket_3?                                            │
│ │  ├─ taxable_income? < bracket_3_limit?                        │
│ │  ├─ taxable_income?                                           │
│ │  │  ├─ income - deductions                                    │
│ │  │  ├─ = 85000 - 12000                                        │
│ │  │  └─ = 73000                                                │
│ │  ├─ = 73000 < 95375                                           │
│ │  └─ (taxable_income? - bracket_2_limit?) × federal_tax_bracket_3?
│ │  ├─ = (73000 - 44725) × 22%                                   │
│ │  ├─ = 28275 × 22%                                             │
│ │  └─ = 6220.50                                                 │
│ ├─ tax_on_bracket_4?                                            │
│ │  ├─ taxable_income? <= bracket_3_limit?                       │
│ │  ├─ = 73000 <= 95375                                          │
│ │  └─ = 0                                                       │
│ ├─ = 1100 + 4047 + 6220.50 + 0                                  │
│ └─ = 11367.50                                                   │
└─────────────────────────────────────────────────────────────────┘
```

## Nested Unless Clause - Only Matched Clause Shown

```
┌─────────────────────────────────────────────────────┐
│ discount_rate = 20%                                 │
├─────────────────────────────────────────────────────┤
│ calculate_discount?                                 │
│ ├─ order_count? > 10                                │
│ ├─ = 15 > 10                                        │
│ └─ = 20%                                            │
└─────────────────────────────────────────────────────┘
```

## Deeply Nested Unless Logic

```
┌──────────────────────────────────────────────────────────────┐
│ final_price = 680 USD                                        │
├──────────────────────────────────────────────────────────────┤
│ base_price? - discount_amount?                               │
│ ├─ = 850 USD - discount_amount?                              │
│ ├─ discount_amount?                                          │
│ │  ├─ base_price? × standard_discount?                       │
│ │  ├─ = 850 USD × standard_discount?                         │
│ │  ├─ standard_discount?                                     │
│ │  │  ├─ customer_tier? == "gold"                            │
│ │  │  └─ = 20%                                               │
│ │  ├─ = 850 USD × 20%                                        │
│ │  └─ = 170 USD                                              │
│ ├─ = 850 USD - 170 USD                                       │
│ └─ = 680 USD                                                 │
└──────────────────────────────────────────────────────────────┘
```

## Key Characteristics

- Box drawing characters: `├─`, `│`, `└─` for tree structure
- Each rule referenced gets indented and shown with tree branches
- Simple fact lookups are substituted inline in calculations
- Rules with logic get expanded showing their evaluation
- Unless clauses: only matched clauses shown with condition (no "unless" keyword)
- Calculations: progressive substitution with `├─ =`
- Results: shown with `└─ =`
- Easy to visually trace execution flow
- Depth is visualized through indentation
