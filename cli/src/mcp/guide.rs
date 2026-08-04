//! Embedded Lemma guides and example specs for MCP tools/resources.

pub const LLMS_TXT: &str = include_str!("../../documentation/llms.txt");
pub const EVALUATE_GUIDE: &str = include_str!("../../documentation/evaluate_guide.txt");

const METHOD: &str = include_str!("../../documentation/guide/05_method.txt");
const SYNTAX: &str = include_str!("../../documentation/guide/10_syntax.txt");
const COMPOSITION: &str = include_str!("../../documentation/guide/20_composition.txt");
const NATURAL_LANGUAGE: &str = include_str!("../../documentation/guide/25_natural_language.txt");
const DATA: &str = include_str!("../../documentation/guide/30_data.txt");
const UNITS: &str = include_str!("../../documentation/guide/40_units.txt");
const RULES: &str = include_str!("../../documentation/guide/50_rules.txt");
const VETO: &str = include_str!("../../documentation/guide/60_veto.txt");
const ANTI_PATTERNS: &str = include_str!("../../documentation/guide/70_anti_patterns.txt");

pub const EXAMPLE_01_COFFEE_ORDER: &str =
    include_str!("../../documentation/examples/01_coffee_order.lemma");
pub const EXAMPLE_02_LIBRARY_FEES: &str =
    include_str!("../../documentation/examples/02_library_fees.lemma");
pub const EXAMPLE_03_RECIPE_SCALING: &str =
    include_str!("../../documentation/examples/03_recipe_scaling.lemma");
pub const EXAMPLE_04_MEMBERSHIP_BENEFITS: &str =
    include_str!("../../documentation/examples/04_membership_benefits.lemma");
pub const EXAMPLE_05_WEATHER_CLOTHING: &str =
    include_str!("../../documentation/examples/05_weather_clothing.lemma");
pub const EXAMPLE_NL_TAX_NET_SALARY: &str =
    include_str!("../../documentation/examples/nl/tax/net_salary.lemma");

/// Guide topics: authoring sections under `cli/documentation/guide/`,
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
    NaturalLanguage,
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
        GuideTopic::NaturalLanguage,
        GuideTopic::AntiPatterns,
        GuideTopic::Evaluate,
        GuideTopic::Full,
    ];

    pub const VALID_LIST: &str = "method, syntax, data, rules, units, veto, composition, natural_language, anti_patterns, evaluate, full";

    pub fn as_str(self) -> &'static str {
        match self {
            GuideTopic::Method => "method",
            GuideTopic::Syntax => "syntax",
            GuideTopic::Data => "data",
            GuideTopic::Rules => "rules",
            GuideTopic::Units => "units",
            GuideTopic::Veto => "veto",
            GuideTopic::Composition => "composition",
            GuideTopic::NaturalLanguage => "natural_language",
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
            GuideTopic::NaturalLanguage => NATURAL_LANGUAGE,
            GuideTopic::AntiPatterns => ANTI_PATTERNS,
            GuideTopic::Evaluate => EVALUATE_GUIDE,
            GuideTopic::Full => LLMS_TXT,
        }
    }
}

/// Example resource: URI path after `lemma://examples/` → body.
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

    #[test]
    fn guide_topic_parse_round_trip() {
        for topic in GuideTopic::ALL {
            assert_eq!(GuideTopic::parse(topic.as_str()), Some(*topic));
        }
        assert!(GuideTopic::parse("temporal").is_none());
        assert!(GuideTopic::parse("").is_none());
    }
}
