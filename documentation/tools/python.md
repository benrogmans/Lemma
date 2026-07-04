---
nav_title: Python
nav_order: 50
---

# Python (coming soon!)

A Python package that brings the Lemma engine to Python is on the way, so Python projects will be able to embed the engine directly. It is not published yet.

Until it lands, embed Lemma through one of the other SDKs, or drive the engine from the command line with the [Lemma CLI](../reference/cli.md). Check back soon.

# Example

```python
from lemma import Engine

engine = Engine()

engine.load("""
    spec pricing
    data count: number
    data price: 10
    rule total: count * price
    rule discount: 0
      unless count >= 10 then 5
      unless count >= 50 then 15
""")

response = engine.run("pricing", {"count": "25"})
```
