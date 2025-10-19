---
layout: default
title: Language Reference
---

# Lemma Language Reference

Quick reference for all operators, units, and types in Lemma.

## Operators

### Arithmetic
| Operator | Description | Example |
|----------|-------------|---------|
| `+` | Addition | `price + tax` |
| `-` | Subtraction | `total - discount` |
| `*` | Multiplication | `price * quantity` |
| `/` | Division | `total / count` |
| `%` | Modulo | `value % 10` |
| `^` | Exponentiation | `base ^ exponent` |

### Comparison
| Operator | Description | Example |
|----------|-------------|---------|
| `>` | Greater than | `age > 18` |
| `<` | Less than | `price < 100` |
| `>=` | Greater or equal | `score >= 70` |
| `<=` | Less or equal | `weight <= 50` |
| `==` | Equal | `status == "active"` |
| `!=` | Not equal | `type != "admin"` |
| `is` | Equal (text-friendly) | `status is "approved"` |
| `is not` | Not equal (text-friendly) | `status is not "cancelled"` |

### Logical
| Operator | Description | Example |
|----------|-------------|---------|
| `and` | Logical AND | `is_valid and not is_blocked` |
| `or` | Logical OR | `is_admin or is_manager` |
| `not` | Logical NOT | `not is_suspended` |
| `have` | Has value | `have user.email` |
| `have not` | Doesn't have value | `have not user.middle_name` |
| `not have` | Doesn't have value | `not have document.signature` |

### Mathematical
| Operator | Description | Example |
|----------|-------------|---------|
| `sqrt` | Square root | `sqrt(value)` or `sqrt value` |
| `sin` | Sine | `sin(angle)` or `sin angle` |
| `cos` | Cosine | `cos(angle)` or `cos angle` |
| `tan` | Tangent | `tan(angle)` or `tan angle` |
| `log` | Natural logarithm | `log(value)` or `log value` |
| `exp` | Exponential | `exp(value)` or `exp value` |
| `abs` | Absolute value | `abs(value)` or `abs value` |
| `floor` | Round down | `floor(value)` or `floor value` |
| `ceil` | Round up | `ceil(value)` or `ceil value` |
| `round` | Round nearest | `round(value)` or `round value` |

Note: Mathematical operators are prefix operators, not functions. Parentheses are optional.

### Unit Conversion
| Operator | Description | Example |
|----------|-------------|---------|
| `in` | Convert units | `weight in pounds` |

## Unit Types

### Money
**Currencies:** `USD`, `EUR`, `GBP`, `JPY`, `CNY`

```lemma
fact price = 100
fact budget = 50000
```

### Mass
**Units:** `kilogram`, `gram`, `milligram`, `pound`, `ounce`

**Plural forms:** `kilograms`, `grams`, `milligrams`, `pounds`, `ounces`

```lemma
fact weight = 10 kilograms
fact portion = 250 grams
```

### Length
**Units:** `kilometer`, `meter`, `centimeter`, `millimeter`, `foot`, `inch`

**Plural forms:** `kilometers`, `meters`, `centimeters`, `millimeters`, `feet`, `inches`

```lemma
fact distance = 5 kilometers
fact height = 180 centimeters
```

### Duration
**Units:** `year`, `month`, `week`, `day`, `hour`, `minute`, `second`

**Plural forms:** `years`, `months`, `weeks`, `days`, `hours`, `minutes`, `seconds`

```lemma
fact workweek = 40 hours
fact vacation = 3 weeks
fact tenure = 5 years
```

### Temperature
**Units:** `celsius`, `fahrenheit`, `kelvin`

```lemma
fact room_temp = 22 celsius
fact body_temp = 98.6 fahrenheit
```

### Volume
**Units:** `liter`, `gallon`

**Plural forms:** `liters`, `gallons`

```lemma
fact capacity = 50 liters
fact tank_size = 15 gallons
```

### Power
**Units:** `watt`, `kilowatt`, `megawatt`, `horsepower`

**Plural forms:** `watts`, `kilowatts`, `megawatts`

```lemma
fact consumption = 1500 watts
fact output = 5 kilowatts
```

### Force
**Units:** `newton`, `kilonewton`, `lbf`

**Plural forms:** `newtons`, `kilonewtons`

```lemma
fact thrust = 500 newtons
```

### Pressure
**Units:** `pascal`, `kilopascal`, `megapascal`, `bar`, `psi`

**Plural forms:** `pascals`, `kilopascals`, `megapascals`, `bars`

```lemma
fact tire_pressure = 32 psi
fact atmospheric = 1 bar
```

### Energy
**Units:** `joule`, `kilojoule`, `megajoule`, `kilowatthour`, `calorie`, `kilocalorie`

**Plural forms:** `joules`, `kilojoules`, `megajoules`, `kilowatthours`, `calories`, `kilocalories`

```lemma
fact energy_used = 100 kilowatthours
fact food_energy = 2000 kilocalories
```

### Frequency
**Units:** `hertz`, `kilohertz`, `megahertz`, `gigahertz`

```lemma
fact cpu_speed = 3.5 gigahertz
fact signal = 100 megahertz
```

### Data Size
**Units:** `byte`, `kilobyte`, `megabyte`, `gigabyte`, `terabyte`

**Plural forms:** `bytes`, `kilobytes`, `megabytes`, `gigabytes`, `terabytes`

```lemma
fact file_size = 10 megabytes
fact storage = 1 terabyte
```

## Type Annotations

Declare expected types without specifying values:

```lemma
fact unknown_date = [date]
fact optional_field = [text]
fact user_age = [number]
fact is_active = [boolean]
fact distance = [length]
fact weight = [mass]
fact duration = [duration]
```

## Boolean Literals

Multiple aliases for readability:

```lemma
true = yes = accept
false = no = reject
```

All are interchangeable:

```lemma
fact is_active = true
fact is_approved = yes
fact can_proceed = accept
```

## Special Expressions

### Veto
Blocks the rule entirely (no valid result):

```lemma
rule result = value
  unless constraint_violated then veto "Error message"
```

Not a boolean - prevents any valid verdict from the rule.

### Have Operator
Checks if a fact has any value:

```lemma
rule has_email = have user.email
rule missing_phone = not have user.phone
```

## Date Formats

ISO 8601 format:

```lemma
fact date_only = 2024-01-15
fact date_time = 2024-01-15T14:30:00Z
fact with_timezone = 2024-01-15T14:30:00+01:00
```

## Regex Patterns

Standard regex syntax between forward slashes:

```lemma
fact email_pattern = /^[\w]+@[\w]+\.[\w]+$/
fact phone_pattern = /\d{3}-\d{3}-\d{4}/
fact code_pattern = /[A-Z]{3}-\d{4}/
```

## Percentages

Literal percentage values (0-100 range):

```lemma
fact tax_rate = 15%
fact discount = 20%
fact completion = 87.5%
```

Use in calculations:

```lemma
rule discount_amount = price * discount_rate
rule after_discount = price * (1 - discount_rate)
```

