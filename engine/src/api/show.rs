//! Show / ShowData / ShowVersion JSON shapes.

use crate::api::types::LemmaType;
use crate::api::value::RuleResultValue;
use crate::literals::Value;
use crate::parsing::ast::DateTimeValue;
use crate::parsing::source::SourceType;
use crate::planning::execution_plan::{
    Show as DomainShow, ShowData as DomainShowData, ShowVersion as DomainShowVersion,
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShowData {
    #[serde(rename = "type")]
    pub lemma_type: LemmaType,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub fill: Option<RuleResultValue>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub suggestion: Option<RuleResultValue>,
    pub needed_by_rules: Vec<String>,
}

impl From<&DomainShowData> for ShowData {
    fn from(data: &DomainShowData) -> Self {
        Self {
            lemma_type: LemmaType::from(&data.lemma_type),
            fill: data.fill.as_ref().map(RuleResultValue::from),
            suggestion: data.suggestion.as_ref().map(RuleResultValue::from),
            needed_by_rules: data.needed_by_rules.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShowVersion {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub effective_from: Option<DateTimeValue>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub effective_to: Option<DateTimeValue>,
}

impl From<&DomainShowVersion> for ShowVersion {
    fn from(version: &DomainShowVersion) -> Self {
        Self {
            effective_from: version.effective_from.clone(),
            effective_to: version.effective_to.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Show {
    pub spec: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub commentary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub effective_from: Option<DateTimeValue>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub effective_to: Option<DateTimeValue>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub versions: Vec<ShowVersion>,
    pub start_line: usize,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source_type: Option<SourceType>,
    pub data: IndexMap<String, ShowData>,
    pub rules: IndexMap<String, LemmaType>,
    pub meta: IndexMap<String, Value>,
}

impl From<&DomainShow> for Show {
    fn from(show: &DomainShow) -> Self {
        Self {
            spec: show.spec.clone(),
            commentary: show.commentary.clone(),
            effective_from: show.effective_from.clone(),
            effective_to: show.effective_to.clone(),
            versions: show.versions.iter().map(ShowVersion::from).collect(),
            start_line: show.start_line,
            source_type: show.source_type.clone(),
            data: show
                .data
                .iter()
                .map(|(name, data)| (name.clone(), ShowData::from(data)))
                .collect(),
            rules: show
                .rules
                .iter()
                .map(|(name, lemma_type)| (name.clone(), LemmaType::from(lemma_type)))
                .collect(),
            meta: show.meta.clone(),
        }
    }
}
