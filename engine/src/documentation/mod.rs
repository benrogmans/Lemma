//! Embedded language guides and example specs.

macro_rules! doc {
    ($path:literal) => {
        concat!(env!("CARGO_MANIFEST_DIR"), $path)
    };
}

pub const LLMS_TXT: &str = concat!(
    include_str!(doc!("/documentation/guide/00_intro.md")),
    "\n\n---\n\n",
    include_str!(doc!("/documentation/guide/05_method.md")),
    "\n\n---\n\n",
    include_str!(doc!("/documentation/guide/10_syntax.md")),
    "\n\n---\n\n",
    include_str!(doc!("/documentation/guide/20_composition.md")),
    "\n\n---\n\n",
    include_str!(doc!("/documentation/guide/30_data.md")),
    "\n\n---\n\n",
    include_str!(doc!("/documentation/guide/40_units.md")),
    "\n\n---\n\n",
    include_str!(doc!("/documentation/guide/50_rules.md")),
    "\n\n---\n\n",
    include_str!(doc!("/documentation/guide/60_veto.md")),
    "\n\n---\n\n",
    include_str!(doc!("/documentation/guide/70_anti_patterns.md")),
    "\n\n---\n\n",
    include_str!(doc!("/documentation/guide/80_footer.md")),
);

pub const EVALUATE_GUIDE: &str = include_str!(doc!("/documentation/evaluate_guide.md"));

const METHOD: &str = include_str!(doc!("/documentation/guide/05_method.md"));
const SYNTAX: &str = include_str!(doc!("/documentation/guide/10_syntax.md"));
const COMPOSITION: &str = include_str!(doc!("/documentation/guide/20_composition.md"));
const DATA: &str = include_str!(doc!("/documentation/guide/30_data.md"));
const UNITS: &str = include_str!(doc!("/documentation/guide/40_units.md"));
const RULES: &str = include_str!(doc!("/documentation/guide/50_rules.md"));
const VETO: &str = include_str!(doc!("/documentation/guide/60_veto.md"));
const ANTI_PATTERNS: &str = include_str!(doc!("/documentation/guide/70_anti_patterns.md"));

pub const EXAMPLE_01_COFFEE_ORDER: &str =
    include_str!(doc!("/documentation/examples/01_coffee_order.lemma"));
pub const EXAMPLE_02_LIBRARY_FEES: &str =
    include_str!(doc!("/documentation/examples/02_library_fees.lemma"));
pub const EXAMPLE_03_RECIPE_SCALING: &str =
    include_str!(doc!("/documentation/examples/03_recipe_scaling.lemma"));
pub const EXAMPLE_04_MEMBERSHIP_BENEFITS: &str =
    include_str!(doc!("/documentation/examples/04_membership_benefits.lemma"));
pub const EXAMPLE_05_WEATHER_CLOTHING: &str =
    include_str!(doc!("/documentation/examples/05_weather_clothing.lemma"));
pub const EXAMPLE_NL_TAX_NET_SALARY: &str =
    include_str!(doc!("/documentation/examples/nl/tax/net_salary.lemma"));

/// Guide topics: authoring sections under `documentation/guide/`,
/// plus `evaluate` (default CS guide) and `full` (complete authoring llms.txt).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuideTopic {
    Method,
    Syntax,
    Data,
    Rules,
    Units,
    Veto,
    Composition,
    AntiPatterns,
    Evaluate,
    Full,
}

impl GuideTopic {
    pub const ALL: &[GuideTopic] = &[
        GuideTopic::Method,
        GuideTopic::Syntax,
        GuideTopic::Data,
        GuideTopic::Rules,
        GuideTopic::Units,
        GuideTopic::Veto,
        GuideTopic::Composition,
        GuideTopic::AntiPatterns,
        GuideTopic::Evaluate,
        GuideTopic::Full,
    ];

    pub const VALID_LIST: &str =
        "method, syntax, data, rules, units, veto, composition, anti_patterns, evaluate, full";

    pub fn as_str(self) -> &'static str {
        match self {
            GuideTopic::Method => "method",
            GuideTopic::Syntax => "syntax",
            GuideTopic::Data => "data",
            GuideTopic::Rules => "rules",
            GuideTopic::Units => "units",
            GuideTopic::Veto => "veto",
            GuideTopic::Composition => "composition",
            GuideTopic::AntiPatterns => "anti_patterns",
            GuideTopic::Evaluate => "evaluate",
            GuideTopic::Full => "full",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|t| t.as_str() == name)
    }

    /// Guide topic content from corresponding fragment.
    pub fn section_text(self) -> &'static str {
        match self {
            GuideTopic::Method => METHOD,
            GuideTopic::Syntax => SYNTAX,
            GuideTopic::Data => DATA,
            GuideTopic::Rules => RULES,
            GuideTopic::Units => UNITS,
            GuideTopic::Veto => VETO,
            GuideTopic::Composition => COMPOSITION,
            GuideTopic::AntiPatterns => ANTI_PATTERNS,
            GuideTopic::Evaluate => EVALUATE_GUIDE,
            GuideTopic::Full => LLMS_TXT,
        }
    }
}

/// Example source: path after `examples/` → body.
pub struct ExampleResource {
    pub path: &'static str,
    pub body: &'static str,
}

pub const EXAMPLE_RESOURCES: &[ExampleResource] = &[
    ExampleResource {
        path: "01_coffee_order.lemma",
        body: EXAMPLE_01_COFFEE_ORDER,
    },
    ExampleResource {
        path: "02_library_fees.lemma",
        body: EXAMPLE_02_LIBRARY_FEES,
    },
    ExampleResource {
        path: "03_recipe_scaling.lemma",
        body: EXAMPLE_03_RECIPE_SCALING,
    },
    ExampleResource {
        path: "04_membership_benefits.lemma",
        body: EXAMPLE_04_MEMBERSHIP_BENEFITS,
    },
    ExampleResource {
        path: "05_weather_clothing.lemma",
        body: EXAMPLE_05_WEATHER_CLOTHING,
    },
    ExampleResource {
        path: "nl/tax/net_salary.lemma",
        body: EXAMPLE_NL_TAX_NET_SALARY,
    },
];

pub fn example_by_path(path: &str) -> Option<&'static str> {
    EXAMPLE_RESOURCES
        .iter()
        .find(|e| e.path == path)
        .map(|e| e.body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn guide_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("documentation/guide")
    }

    fn concat_guide_fragments() -> String {
        let mut entries = fs::read_dir(guide_dir())
            .expect("BUG: documentation/guide/ must exist")
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.extension()?.to_str()? == "md" {
                    Some(path)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        entries.sort();
        let mut content = String::new();
        for (i, path) in entries.iter().enumerate() {
            if i > 0 {
                content.push_str("\n\n---\n\n");
            }
            content.push_str(
                &fs::read_to_string(path)
                    .unwrap_or_else(|e| panic!("BUG: read {}: {e}", path.display())),
            );
        }
        content
    }

    #[test]
    fn guide_topic_parse_round_trip() {
        for topic in GuideTopic::ALL {
            assert_eq!(GuideTopic::parse(topic.as_str()), Some(*topic));
        }
        assert!(GuideTopic::parse("temporal").is_none());
        assert!(GuideTopic::parse("").is_none());
    }

    #[test]
    fn full_guide_matches_concatenated_fragments() {
        assert_eq!(GuideTopic::Full.section_text(), concat_guide_fragments());
    }

    #[test]
    fn full_guide_does_not_embed_evaluate_guide() {
        assert!(!GuideTopic::Full
            .section_text()
            .contains("**Evaluating loaded specs**"));
    }
}
