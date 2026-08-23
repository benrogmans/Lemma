**Mandatory spec opening order:**

```
spec <name> [<effective>]
[""" commentary: optional, but if present must be HERE """]
meta ...
uses ...
data ...
rule ...
```

Commentary after `uses` or `data` is invalid. Optional `meta key: value` after commentary (provenance, not policy). `rule name:` with the body on the next indented line. No `#`, `//`, `--` comments. Use descriptive names. Put user explanations outside code fences, never inside ` ```lemma ` blocks.

**Gotchas (parse errors)**

- No `or` operator. Disjunction via `unless` chains or separate boolean rules.
- Constraints (`-> help`, `-> option`, `-> minimum`, etc.) apply to `data` only. Rules have no constraints.
