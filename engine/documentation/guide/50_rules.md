**Rules and unless: last matching clause wins**

Default expression, then `unless <condition> then <result>`. Source order; **bottommost match wins**. General first, specific last. Snake_case names; boolean predicates (`is_eligible`, `can_ship`). Named pipeline rules, no opaque mega-expressions.

**Domain-principle default**

Default is not "prefer yes/no." It is the answer **in principle for this rule's domain**. Experts must read top-to-bottom as: "In principle X; unless Y, then Z."

1. Name the question (`can_ship` → "Can we ship this order?").
2. Before special cases, what is true in principle? That is the default: from *this* rule's domain, not optimism or "start true and subtract failures."
3. What positive facts change the answer? Those are `unless` conditions, not negated flips (`unless not ready then no`).
4. Write `rule name: <principle> unless <positive conditions> then <exception>`.

Examples by domain: shipping often earned (`no` unless grant); discount often `0%`; fees use the policy's usual fee, not "free unless expensive."

Forbidden: double denial (`yes` / `unless not … then no`); fail-each-check cascades; invented `*_compliant` default-yes helpers; a single `no` / `unless … then yes` when the condition is the whole answer (see **Anti-patterns**). In `and` chains, do not mix a bare name with `not` / `is false` (WRONG: `x and not y`); use parallel probes (`not x and y is true`, or `x is false and y is true`) or parentheses. Unary `not x` alone is fine.

**Overlapping unless (last wins)**

```lemma
spec vip_discount

data qty: number
  -> minimum 0
  -> help "How many items?"

data is_vip: boolean
  -> suggest false
  -> help "Is the customer a VIP?"


rule discount:
  0%
  unless qty >= 10 then 10%
  unless qty >= 50 then 20%
  unless is_vip    then 25%
```

VIP ordering 75 items gets **25%** (not 20%): both `qty >= 50` and `is_vip` match; bottommost wins.
