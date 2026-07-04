---
nav_title: C# / .NET
nav_order: 60
---

# C# / .NET (coming soon!)

A NuGet package that brings the Lemma engine to .NET is on the way, so C# and other .NET projects will be able to embed the engine directly. It is not published yet.

Until it lands, embed Lemma through one of the other SDKs, or drive the engine from the command line with the [Lemma CLI](../reference/cli.md). Check back soon.

# Example

```csharp
using Lemma;

var engine = new Engine();

engine.Load("""
    spec pricing
    data count: number
    data price: 10
    rule total: count * price
    rule discount: 0
      unless count >= 10 then 5
      unless count >= 50 then 15
    """);

var response = engine.Run("pricing", new() { ["count"] = "25" });
```
