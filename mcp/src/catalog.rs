use serde::Serialize;

use lemma::documentation::{example_by_path, GuideTopic, EVALUATE_GUIDE, EXAMPLE_RESOURCES};

use crate::error::ResourceError;

#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceDefinition {
    pub uri: String,
    pub name: String,
    #[serde(rename = "mimeType")]
    pub mime_type: &'static str,
    pub description: String,
}

pub fn list_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "evaluate",
            description: "Evaluate rules. Pass `rule` to target one rule; omit for all. For human intake: call guide (default = evaluate guide; not topic full), then list, show once, evaluate. missing_data lines include name, type, and help. Primary loop: after each user turn, bind every field that utterance decides (entailments), re-evaluate; ask at most one open topic-question when something remains. Never ask the user what the policy means. Never dispose interpretation as truth; use “should” when a judgment call cannot be answered. When the rule answers, present details+answer in domain language for user verify (no tooling jargon to the user) before treating as done. No questionnaire dumps. No re-call show between asks. Do not dump every show data field into evaluate. Returns display values, unit maps, reasoning, and missing_data when inputs are still needed.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "spec": {
                        "type": "string",
                        "description": "Spec set id, e.g. pricing"
                    },
                    "rule": {
                        "type": "string",
                        "description": "Optional: name of a specific rule to evaluate. Omit to evaluate all rules."
                    },
                    "data": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional data values as 'name=value' (e.g. ['price=100', 'measure=5']). Partial is fine.",
                        "default": []
                    },
                    "effective": {
                        "type": "string",
                        "description": "Optional: evaluate at a specific effective datetime (e.g. '2026', '2026-03', '2026-03-04', '2026-03-04T10:30:00Z')"
                    }
                },
                "required": ["spec"]
            }),
        },
        ToolDefinition {
            name: "list",
            description: "List loaded specs by repository (name, effective_from, effective_to). Call this first when you do not already know the exact spec name. Do not invent or guess spec names. For human intake call guide (default evaluate guide), then show once and evaluate.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: "show",
            description: "Return JSON Show for a spec: data catalog (types, constraints, suggestions, units, help) and rule output types. Call once after list. Static interface — not a required-input list, not a questionnaire, not something to re-call between evaluate/ask turns. Human intake: call guide (default = evaluate guide).",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "spec": {
                        "type": "string",
                        "description": "Spec set id, e.g. pricing"
                    },
                    "effective": {
                        "type": "string",
                        "description": "Optional: show at a specific effective datetime"
                    }
                },
                "required": ["spec"]
            }),
        },
        ToolDefinition {
            name: "source",
            description: "Return formatted Lemma source. Pass `repository` (e.g. `lemma` for embedded units stdlib) for the whole repo, or `spec` for a workspace spec.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "repository": {
                        "type": "string",
                        "description": "Repository qualifier (e.g. lemma). When set, returns formatted source for the entire repository."
                    },
                    "spec": {
                        "type": "string",
                        "description": "Workspace spec set id (when repository is omitted)"
                    },
                    "effective": {
                        "type": "string",
                        "description": "Optional: get source at a specific effective datetime"
                    }
                }
            }),
        },
        ToolDefinition {
            name: "check",
            description: "Validate Lemma sources (does not load). On success confirms syntax is valid. Call add_spec to load after check passes. On failure returns structured diagnostics (kind, message, suggestion, source line/column). Sources resolve cross-file `uses` within the batch. A leading `@` label loads as a dependency. Lemma has no `#` or `//` comments; commentary is valid only as a docstring immediately after the `spec` line. Before drafting new specs, call guide with topic full.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "sources": {
                        "type": "array",
                        "items": {
                            "type": "array",
                            "items": { "type": "string" },
                            "minItems": 2,
                            "maxItems": 2
                        },
                        "description": "Array of [label, code] pairs. Label is the source path (e.g. 'pricing.lemma') or a dependency identifier (e.g. '@org/repo')."
                    }
                },
                "required": ["sources"]
            }),
        },
        ToolDefinition {
            name: "guide",
            description: "Return a Lemma guide. Omit topic for the evaluate guide (CS intake with loaded specs — default). Pass topic full only when authoring new Lemma specs (complete authoring guide). Other topics are authoring sections: method, syntax, data, rules, units, veto, composition, natural_language, anti_patterns; topic evaluate is the same as the default.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "topic": {
                        "type": "string",
                        "enum": ["method", "syntax", "data", "rules", "units", "veto", "composition", "natural_language", "anti_patterns", "evaluate", "full"],
                        "description": "Optional. Omit for evaluate guide. Use full only when authoring new specs."
                    }
                }
            }),
        },
    ]
}

pub fn list_resources() -> Vec<ResourceDefinition> {
    let mut resources = vec![ResourceDefinition {
        uri: "lemma://guide".to_string(),
        name: "Lemma evaluate guide".to_string(),
        mime_type: "text/plain",
        description: "Default evaluate guide for CS intake. Same as guide tool with no topic. Use lemma://guide/full only when authoring.".to_string(),
    }];
    for topic in GuideTopic::ALL {
        resources.push(ResourceDefinition {
            uri: format!("lemma://guide/{}", topic.as_str()),
            name: format!("Lemma guide: {}", topic.as_str()),
            mime_type: "text/plain",
            description: format!("Guide section '{}'", topic.as_str()),
        });
    }
    for example in EXAMPLE_RESOURCES {
        resources.push(ResourceDefinition {
            uri: format!("lemma://examples/{}", example.path),
            name: example.path.to_string(),
            mime_type: "text/plain",
            description: format!("Example Lemma source: {}", example.path),
        });
    }
    resources
}

pub fn read_resource(uri: &str) -> Result<&'static str, ResourceError> {
    if uri == "lemma://guide" {
        return Ok(EVALUATE_GUIDE);
    }
    if let Some(topic_name) = uri.strip_prefix("lemma://guide/") {
        let topic = GuideTopic::parse(topic_name).ok_or_else(|| {
            ResourceError::UnknownUri(format!(
                "Unknown guide topic '{topic_name}'. Valid: {}",
                GuideTopic::VALID_LIST
            ))
        })?;
        return Ok(topic.section_text());
    }
    if let Some(path) = uri.strip_prefix("lemma://examples/") {
        return example_by_path(path)
            .ok_or_else(|| ResourceError::UnknownUri(format!("Unknown example resource: {uri}")));
    }
    Err(ResourceError::UnknownUri(format!(
        "Unknown resource URI: {uri}. Use resources/list."
    )))
}
