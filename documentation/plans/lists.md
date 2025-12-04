---
layout: default
title: Lists
---

# list Facts Implementation Plan

## Overview

Extend Lemma to support facts that hold listple values (lists) with declarative operators for aggregation and querying. This enables working with collections of data (salaries, employees, transactions) within documents using declarative list operations.

## Design

### Simple List-Value Facts

```lemma
doc payroll
fact salaries = [list money]

fact salaries = 10 eur, 20 eur, 3 usd
```

### List-Doc References

```lemma
doc employee
fact name = [text]
fact salary = [money]

doc staff
fact members = list doc employee
```

### Indexed Access

```lemma
doc staff
fact members = list doc employee

fact members.0.name = "Bob"
fact members.0.salary = 80000 usd

fact members.1.name = "Alice"
fact members.1.salary = 75000 usd

fact members.alice.name = "Alice"
fact members.bob.name = "Bob"
```

Index can be any alphanumeric value (numeric or text identifier).

### Declarative List Operations

```lemma
rule total_salaries = sum salaries
rule avg_salary = avg salaries
rule highest_salary = max salaries
rule lowest_salary = min salaries
rule salary_count = count salaries

rule total_staff_cost = sum members.salary
rule avg_staff_salary = avg members.salary
rule staff_count = count members
```

### Filtering with Where

```lemma
rule high_earners = count members where members.salary > 80000 usd
rule engineering_total = sum members.salary where members.department is "Engineering"
rule senior_staff = find members where members.years_experience >= 5
```

## Key Features

1. **List-value syntax** - `[list type]` or comma-separated values
2. **Indexed access** - `fact_name.index.field` notation
3. **Declarative operators** - `sum`, `avg`, `min`, `max`, `count`, `find`
4. **Filtering** - `where` clause for conditional operations
5. **Type safety** - Operations validate types (can't sum text fields)
6. **Nested structures** - List-facts of docs with their own fields

## Implementation Phases

*To be detailed...*

## Timeline

*To be estimated...*

## Key Design Points

1. **No object declarations** - List-facts are just collections, accessed via indexing
2. **Flexible indexing** - Numeric or alphanumeric identifiers
3. **Doc references** - List-facts can hold doc references
4. **Declarative operations** - No imperative loops, just operations on collections
5. **Type preservation** - Operations return appropriate types (sum returns money if input is money)

