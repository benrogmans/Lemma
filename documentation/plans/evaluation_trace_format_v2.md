# Evaluation Trace Format v2 (Tree-based)

## Simple Calculation

```
┌────────────┬────────┐
│ Fact       │ Value  │
├────────────┼────────┤
│ income     │ 85,000 │
│ deductions │ 12,000 │
│ state      │ "CA"   │
└────────────┴────────┘

┌─────────────────────────┐
│ taxable_income = 73,000 │
├─────────────────────────┤
│ income - deductions     │
│ ├─ = 85,000 - 12,000    │
│ └> = 73,000             │
└─────────────────────────┘
```

## With Referenced Rules

```
┌───────────────────────────────────────────┐
│ tax_on_bracket_1 = 1,100                  │
├───────────────────────────────────────────┤
│ bracket_1_limit? * federal_tax_bracket_1? │
│ ├─ bracket_1_limit?                       │
│ │  └> 11,000                              │
│ ├─ federal_tax_bracket_1?                 │
│ │  └> 10%                                 │
│ ├─ = 11,000 * 10%                         │
│ └> = 1,100                                │
└───────────────────────────────────────────┘
```

## Unless Clauses

```
rule tax_on_bracket_3 = (bracket_3_limit? - bracket_2_limit?) * federal_tax_bracket_3?
  unless taxable_income? < bracket_3_limit? then (taxable_income? - bracket_2_limit?) * federal_tax_bracket_3?
  unless taxable_income? < bracket_2_limit? then 0

┌───────────────────────────────────────────────────────────────────┐
│ tax_on_bracket_3 = 6,220.50                                       │
├───────────────────────────────────────────────────────────────────┤
│ taxable_income? < bracket_3_limit?                                │
│ ├─ taxable_income?                                                │
│ │  ├─ income - deductions                                         │
│ │  ├─ = 85,000 - 12,000                                           │
│ │  └> = 73,000                                                    │
│ ├─ = 73,000 < 95,375                                              │
│ └> (taxable_income? - bracket_2_limit?) * federal_tax_bracket_3?  │
│    ├─ = (73,000 - 44,725) * 22%                                   │
│    └> = 6,220.50                                                  │
│ ×─ taxable_income? < bracket_2_limit?                             │
│    └─ = 73,000 < 44,725                                           │
└───────────────────────────────────────────────────────────────────┘

┌────────────────────────────────────────────────┐
│ tax_on_bracket_4 = 0                           │
├────────────────────────────────────────────────┤
│ taxable_income? <= bracket_3_limit?            │
│ ├─ = 73,000 <= 95,375                          │
│ └> 0                                           │
└────────────────────────────────────────────────┘
```

## Improved Unless Clauses (Clearer Logic)

```
rule tax_on_bracket_3 = 0
  unless taxable_income? >= bracket_2_limit? then (taxable_income? - bracket_2_limit?) * federal_tax_bracket_3?
  unless taxable_income? >= bracket_3_limit? then (bracket_3_limit? - bracket_2_limit?) * federal_tax_bracket_3?

┌───────────────────────────────────────────────────────────────────┐
│ tax_on_bracket_3 = 6,220.50                                       │
├───────────────────────────────────────────────────────────────────┤
│ taxable_income? >= bracket_2_limit?                               │
│ ├─ taxable_income?                                                │
│ │  ├─ income - deductions                                         │
│ │  ├─ = 85,000 - 12,000                                           │
│ │  └> = 73,000                                                    │
│ ├─ = 73,000 >= 44,725                                             │
│ └> (taxable_income? - bracket_2_limit?) * federal_tax_bracket_3?  │
│    ├─ = (73,000 - 44,725) * 22%                                   │
│    └> = 6,220.50                                                  │
│ ×─ taxable_income? >= bracket_3_limit?                            │
│    └─ = 73,000 >= 95,375                                          │
└───────────────────────────────────────────────────────────────────┘
```

## Complex Multi-Rule Calculation

```
┌───────────────────────────────────────────────────────────────────────────────┐
│ total_federal_tax = 11_367.50                                                 │
├───────────────────────────────────────────────────────────────────────────────┤
│ tax_on_bracket_1? + tax_on_bracket_2? + tax_on_bracket_3? + tax_on_bracket_4? |
│ ├─ tax_on_bracket_1?                                                          │
│ │  ├─ bracket_1_limit? * federal_tax_bracket_1?                               │
│ │  ├─ = 11,000 * 10%                                                          │
│ │  └> = 1,100                                                                 │
│ ├─ tax_on_bracket_2?                                                          │
│ │  ├─ (bracket_2_limit? - bracket_1_limit?) * federal_tax_bracket_2?          |
│ │  ├─ = (44,725 - 11,000) * 12%                                               │
│ │  └> = 4_047                                                                 │
│ ├─ tax_on_bracket_3?                                                          │
│ │  ├─ taxable_income? < bracket_3_limit?                                      │
│ │  ├─ taxable_income?                                                         │
│ │  │  ├─ income - deductions                                                  │
│ │  │  ├─ = 85,000 - 12,000                                                    │
│ │  │  └> = 73,000                                                             │
│ │  ├─ = 73,000 < 95,375                                                       │
│ │  └> (taxable_income? - bracket_2_limit?) * federal_tax_bracket_3?           |
│ │     ├─ = (73,000 - 44,725) * 22%                                            │
│ │     └> = 6,220.50                                                           │
│ ├─ tax_on_bracket_4?                                                          │
│ │  ├─ taxable_income? <= bracket_3_limit?                                     │
│ │  ├─ = 73,000 <= 95,375                                                      │
│ │  └> 0                                                                       │
│ ├─ = 1,100 + 4_047 + 6,220.50 + 0                                             │
│ └> = 11_367.50                                                                │
└───────────────────────────────────────────────────────────────────────────────┘
```

## Nested Unless Clause - Only Matched Clause Shown

```
┌─────────────────────────────────────────────────────┐
│ discount_rate = 20%                                 │
├─────────────────────────────────────────────────────┤
│ calculate_discount?                                 │
│ ├─ order_count? > 10                                │
│ ├─ = 15 > 10                                        │
│ └> 20%                                              │
└─────────────────────────────────────────────────────┘
```

## Deeply Nested Unless Logic

```
┌──────────────────────────────────────────────────────────────┐
│ final_price = 807.50 USD                                     │
├──────────────────────────────────────────────────────────────┤
│ base_price? - discount_amount?                               │
│ ├─ base_price?                                               │
│ │  └> 850 USD                                                │
│ ├─ discount_amount?                                          │
│ │  ├─ base_price? * standard_discount?                       │
│ │  ├─ = 850 USD * standard_discount?                         │
│ │  ├─ standard_discount?                                     │
│ │  │  ├─ customer_tier?                                      │
│ │  │  │  └> "bronze"                                         │
│ │  │  ├─ customer_tier? == "bronze"                          │
│ │  │  ├─ = "bronze" == "bronze"                              │
│ │  │  └> 5%                                                  │
│ │  │  ×─ customer_tier? == "silver"                          │
│ │  │     └─ = "bronze" == "silver"                           │
│ │  │  ×─ customer_tier? == "gold"                            │
│ │  │     └─ = "bronze" == "gold"                             │
│ │  ├─ = 850 USD * 5%                                         │
│ │  └> = 42.50 USD                                            │
│ ├─ = 850 USD - 42.50 USD                                     │
│ └> = 807.50 USD                                              │
└──────────────────────────────────────────────────────────────┘
```

## Loan Pricing System example

This example demonstrates a comprehensive loan pricing system with multiple nested calculations, complex unless clause logic, and real-world business rules.

### Example Document

```
doc examples/loan_pricing
"""
Loan Pricing and Approval System

A comprehensive example demonstrating:
- Nested rule calculations with multiple levels of dependencies
- Complex unless clause logic with multiple conditions
- Real-world business rules for loan pricing
- Risk-based pricing tiers
"""

fact loan_amount = 250000 USD
fact credit_score = 720
fact annual_income = 95000
fact debt_to_income_ratio = 0.28
fact loan_term_years = 30
fact property_type = "primary_residence"
fact down_payment_percent = 20%
fact is_first_time_buyer = true
fact has_co_signer = false
fact employment_years = 5

rule base_interest_rate = 6.5%
  unless credit_score >= 740 then 6.0%
  unless credit_score >= 760 then 5.75%
  unless credit_score >= 780 then 5.5%
  unless credit_score < 620 then 8.5%

rule down_payment_adjustment = 0%
  unless down_payment_percent < 20% then 0.5%
  unless down_payment_percent < 10% then 1.0%
  unless down_payment_percent < 5% then 1.5%

rule property_type_adjustment = 0%
  unless property_type == "investment" then 0.75%
  unless property_type == "second_home" then 0.5%

rule first_time_buyer_discount = 0%
  unless is_first_time_buyer then -0.25%

rule co_signer_discount = 0%
  unless has_co_signer then -0.15%

rule risk_tier = "standard"
  unless credit_score >= 760 and debt_to_income_ratio <= 0.36 then "premium"
  unless credit_score < 640 or debt_to_income_ratio > 0.43 then "high_risk"
  unless credit_score < 600 then "declined"

rule risk_adjustment = 0%
  unless risk_tier? == "premium" then -0.25%
  unless risk_tier? == "high_risk" then 1.0%
  unless risk_tier? == "declined" then veto "Loan cannot be approved"

rule employment_stability_adjustment = 0%
  unless employment_years >= 2 then 0%
  unless employment_years < 1 then 0.5%

rule final_interest_rate = base_interest_rate? + 
                           down_payment_adjustment? + 
                           property_type_adjustment? + 
                           first_time_buyer_discount? + 
                           co_signer_discount? + 
                           risk_adjustment? + 
                           employment_stability_adjustment?

rule monthly_interest_rate = final_interest_rate? / 12

rule loan_term_months = loan_term_years * 12

rule monthly_payment = loan_amount * 
                       (monthly_interest_rate? * (100% + monthly_interest_rate?) ^ loan_term_months?) / 
                       ((100% + monthly_interest_rate?) ^ loan_term_months? - 1)

rule total_interest_paid = (monthly_payment? * loan_term_months?) - loan_amount

rule total_loan_cost = loan_amount + total_interest_paid?

rule payment_to_income_ratio = monthly_payment? / (annual_income / 12)

rule is_affordable = false
  unless payment_to_income_ratio? <= 0.28 then true
  unless payment_to_income_ratio? <= 0.31 then true

rule approval_status = "pending"
  unless risk_tier? == "declined" then "declined"
  unless is_affordable? then "approved"
  unless payment_to_income_ratio? > 0.36 then "declined"

rule loan_summary = "Standard loan"
  unless risk_tier? == "premium" then "Premium rate loan"
  unless is_first_time_buyer then "First-time buyer program"
  unless approval_status? == "declined" then "Loan declined"
```

### Key Evaluation Traces

#### Risk Tier Calculation with Multiple Conditions

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ risk_tier = "standard"                                                       │
├──────────────────────────────────────────────────────────────────────────────┤
│ └> "standard"                                                                │
│ ×─ credit_score >= 760 and debt_to_income_ratio <= 0.36                      │
│    ├─ credit_score >= 760                                                    │
│    │  ├─ credit_score                                                        │
│    │  │  └> 720                                                              │
│    │  └─ = 720 >= 760                                                        │
│    ├─ debt_to_income_ratio <= 0.36                                           │
│    │  ├─ debt_to_income_ratio                                                │
│    │  │  └> 0.28                                                             │
│    │  └─ = 0.28 <= 0.36                                                      │
│    └─ = false and true                                                       │
│ ×─ credit_score < 640 or debt_to_income_ratio > 0.43                         │
│    ├─ credit_score < 640                                                     │
│    │  └─ = 720 < 640                                                         │
│    ├─ debt_to_income_ratio > 0.43                                            │
│    │  └─ = 0.28 > 0.43                                                       │
│    └─ = false or false                                                       │
│ ×─ credit_score < 600                                                        │
│    └─ = 720 < 600                                                            │
└──────────────────────────────────────────────────────────────────────────────┘
```

#### Final Interest Rate with Multiple Adjustments

```
┌────────────────────────────────────────────────────────────────────────────────┐
│ final_interest_rate = 6.25%                                                    │
├────────────────────────────────────────────────────────────────────────────────┤
│ base_interest_rate? + down_payment_adjustment?                                 |
|    + property_type_adjustment? + first_time_buyer_discount?                    |
|    + co_signer_discount? + risk_adjustment? + employment_stability_adjustment? |
│ ├─ base_interest_rate?                                                         │
│ │  └> 6.5%                                                                     │
│ │  ×─ credit_score >= 740                                                      │
│ │     ├─ credit_score                                                          │
│ │     │  └> 720                                                                │
│ │     └─ = 720 >= 740                                                          │
│ │  ×─ credit_score >= 760                                                      │
│ │     └─ = 720 >= 760                                                          │
│ │  ×─ credit_score >= 780                                                      │
│ │     └─ = 720 >= 780                                                          │
│ │  ×─ credit_score < 620                                                       │
│ │     └─ = 720 < 620                                                           │
│ ├─ down_payment_adjustment?                                                    │
│ │  └> 0%                                                                       │
│ │  ×─ down_payment_percent < 20%                                               │
│ │     ├─ down_payment_percent                                                  │
│ │     │  └> 20%                                                                │
│ │     └─ = 20% < 20%                                                           │
│ │  ×─ down_payment_percent < 10%                                               │
│ │     └─ = 20% < 10%                                                           │
│ │  ×─ down_payment_percent < 5%                                                │
│ │     └─ = 20% < 5%                                                            │
│ ├─ property_type_adjustment?                                                   │
│ │  └> 0%                                                                       │
│ │  ×─ property_type == "investment"                                            │
│ │     ├─ property_type                                                         │
│ │     │  └> "primary_residence"                                                │
│ │     └─ = "primary_residence" == "investment"                                 │
│ │  ×─ property_type == "second_home"                                           │
│ │     └─ = "primary_residence" == "second_home"                                │
│ ├─ first_time_buyer_discount?                                                  │
│ │  ├─ is_first_time_buyer                                                      │
│ │  │  └> true                                                                  │
│ │  └> -0.25%                                                                   │
│ ├─ co_signer_discount?                                                         │
│ │  └> 0%                                                                       │
│ │  ×─ has_co_signer                                                            │
│ │     └> false                                                                 │
│ ├─ risk_adjustment?                                                            │
│ │  └> 0%                                                                       │
│ │  ×─ risk_tier? == "premium"                                                  │
│ │     ├─ risk_tier?                                                            │
│ │     │  └> "standard"                                                         │
│ │     └─ = "standard" == "premium"                                             │
│ │  ×─ risk_tier? == "high_risk"                                                │
│ │     ├─ risk_tier?                                                            │
│ │     │  └> "standard"                                                         │
│ │     └─ = "standard" == "high_risk"                                           │
│ │  ×─ risk_tier? == "declined"                                                 │
│ │     ├─ risk_tier?                                                            │
│ │     │  └> "standard"                                                         │
│ │     └─ = "standard" == "declined"                                            │
│ ├─ employment_stability_adjustment?                                            │
│ │  ├─ employment_years >= 2                                                    │
│ │  │  ├─ employment_years                                                      │
│ │  │  │  └> 5                                                                  │
│ │  │  └─ = 5 >= 2                                                              │
│ │  └> 0%                                                                       │
│ │  ×─ employment_years < 1                                                     │
│ │     └─ = 5 < 1                                                               │
│ ├─ = 6.5% + 0% + 0% + (-0.25%) + 0% + 0% + 0%                                  │
│ └> = 6.25%                                                                     │
└────────────────────────────────────────────────────────────────────────────────┘
```

#### Monthly Payment Calculation (Complex Formula)

```
┌─────────────────────────────────────────────────────────────────────────────────────┐
│ monthly_payment = 1,540.23 USD                                                      │
├─────────────────────────────────────────────────────────────────────────────────────┤
│ loan_amount                                                                         |
|    * (monthly_interest_rate? * (100% + monthly_interest_rate?) ^ loan_term_months?) |
|    / ((100% + monthly_interest_rate?) ^ loan_term_months? - 1)                      │
│ ├─ loan_amount                                                                      │
│ │  └> 250,000 USD                                                                   │
│ ├─ monthly_interest_rate?                                                           │
│ │  ├─ final_interest_rate? / 12                                                     │
│ │  │  ├─ final_interest_rate?                                                       │
│ │  │  │  └> 6.25%                                                                   │
│ │  │  ├─ = 6.25% / 12                                                               │
│ │  │  └> = 0.52%                                                                    │
│ │  └> 0.52%                                                                         │
│ ├─ loan_term_months?                                                                │
│ │  ├─ loan_term_years * 12                                                          │
│ │  │  ├─ loan_term_years                                                            │
│ │  │  │  └> 30                                                                      │
│ │  │  ├─ = 30 * 12                                                                  │
│ │  │  └> = 360                                                                      │
│ │  └> 360                                                                           │
│ ├─ = 250,000 USD * (0.52% * (100% + 0.52%) ^ 360) / ((100% + 0.52%) ^ 360 - 1)      │
│ └> = 1,540.23 USD                                                                   │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

#### Payment to Income Ratio with Nested Rule References

```
┌───────────────────────────────────────────────────────────────────────────────┐
│ payment_to_income_ratio = 0.194                                               │
├───────────────────────────────────────────────────────────────────────────────┤
│ monthly_payment? / (annual_income / 12)                                       │
│ ├─ monthly_payment?                                                           │
│ │  └> 1,540.23 USD                                                            │
│ ├─ annual_income / 12                                                         │
│ │  ├─ annual_income                                                           │
│ │  │  └> 95,000                                                               │
│ │  ├─ = 95,000 / 12                                                           │
│ │  └> = 7,916.67                                                              │
│ ├─ = 1,540.23 USD / 7,916.67                                                  │
│ └> = 0.194                                                                    │
└───────────────────────────────────────────────────────────────────────────────┘
```

## Key Characteristics

- Box drawing characters: `├─`, `│`, `└─` for tree structure
- Each rule referenced gets indented and shown with tree branches
- Simple fact lookups are substituted inline in calculations
- Rules with logic get expanded showing their evaluation
- Unless clauses: reversed operations to match the source code (check evaluator to learn about early returns)
- Unless clauses: matched statements are fully expanded; non-matched clauses shown with `×─` and expanded to show condition evaluation (the `×─` marker indicates the condition evaluated to false)
- Calculations: progressive substitution with `├─ =`
- Results and unless clause values: shown with `└> `
- When results and unless clause values are the result of a computation, they are shown as `└> =`
- Easy to visually trace execution flow
- Depth is visualized through indentation