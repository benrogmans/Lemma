use comfy_table::{presets::UTF8_FULL, Cell, CellAlignment, Table};
use lemma::{Fact, Response, RuleResult};
use std::collections::HashSet;

pub struct Formatter {}

impl Default for Formatter {
    fn default() -> Self {
        Self::new()
    }
}

impl Formatter {
    pub fn new() -> Self {
        Self {}
    }

    pub fn format_response(&self, response: &Response, _raw: bool) -> String {
        let mut output = String::new();

        if !response.facts.is_empty() {
            output.push_str(&self.format_facts_table(&response.facts));
            output.push('\n');
        }

        let mut sorted_results = response.results.clone();
        sorted_results.sort_by_key(|result| {
            result
                .rule
                .source_location
                .as_ref()
                .map(|loc| loc.span.start)
                .unwrap_or(usize::MAX)
        });

        // Track which rules have been expanded across the entire response
        let mut expanded_rules = HashSet::new();

        for result in &sorted_results {
            output.push_str(&self.format_rule_result_with_cache(result, &mut expanded_rules));
            output.push('\n');
        }

        output
    }

    fn format_facts_table(&self, facts: &[Fact]) -> String {
        let mut table = Table::new();
        table.load_preset(UTF8_FULL);
        table.set_header(vec![
            Cell::new("Fact").set_alignment(CellAlignment::Left),
            Cell::new("Value").set_alignment(CellAlignment::Left),
        ]);

        for fact in facts {
            let value_str = if let Some(doc_name) = &fact.document_reference {
                format!("doc {}", doc_name)
            } else {
                fact.value
                    .as_ref()
                    .map(|v| v.to_string())
                    .unwrap_or_default()
            };
            table.add_row(vec![
                Cell::new(&fact.name).set_alignment(CellAlignment::Left),
                Cell::new(value_str).set_alignment(CellAlignment::Right),
            ]);
        }

        table.to_string()
    }

    fn format_rule_result_with_cache(
        &self,
        result: &RuleResult,
        expanded_rules: &mut HashSet<String>,
    ) -> String {
        let header = match &result.result {
            lemma::OperationResult::Value(value) => {
                format!("{} = {}", result.rule.name, value)
            }
            lemma::OperationResult::Veto(msg) => {
                if let Some(msg) = msg {
                    format!("{} = Veto: {}", result.rule.name, msg)
                } else {
                    format!("{} = Veto", result.rule.name)
                }
            }
        };

        // Mark this rule as expanded
        expanded_rules.insert(result.rule.name.clone());

        // Build the content (without prefix - table will provide border)
        let content = if let Some(proof) = &result.proof {
            self.format_proof_node(&proof.tree, "", expanded_rules)
        } else {
            match &result.result {
                lemma::OperationResult::Value(_) => String::new(),
                lemma::OperationResult::Veto(msg) => {
                    if let Some(msg) = msg {
                        format!("└> Veto: {}", msg)
                    } else {
                        "└> Veto".to_string()
                    }
                }
            }
        };

        // Use comfy-table with single column and custom style
        let mut table = Table::new();
        table.load_preset(UTF8_FULL);

        // Customize to use solid lines instead of dashed for separators
        table.set_style(comfy_table::TableComponent::MiddleIntersections, '┼');
        table.set_style(comfy_table::TableComponent::HorizontalLines, '─');

        // Add header row
        table.add_row(vec![header]);

        // Add content row if not empty
        if !content.trim().is_empty() {
            table.add_row(vec![content.trim_end()]);
        }

        table.to_string()
    }

    fn format_proof_node(
        &self,
        node: &lemma::proof::ProofNode,
        prefix: &str,
        expanded_rules: &mut HashSet<String>,
    ) -> String {
        use lemma::proof::ProofNode;

        match node {
            ProofNode::Value { value, .. } => {
                format!("{}└> {}\n", prefix, value)
            }

            ProofNode::Computation {
                original_expression,
                expression,
                result,
                operands,
                ..
            } => {
                let mut output = String::new();

                // 1. Show original expression
                output.push_str(&format!("{}{}\n", prefix, original_expression));

                // 2. Expand all rule references (only once)
                let rule_refs = Self::collect_rule_references(operands);
                for rule_ref in rule_refs {
                    output.push_str(&self.format_rule_reference_expansion(
                        rule_ref,
                        prefix,
                        expanded_rules,
                    ));
                }

                // 3. Show substituted calculation
                output.push_str(&format!("{}├─ = {}\n", prefix, expression));

                // 4. Show final result
                output.push_str(&format!("{}└> = {}\n", prefix, result));

                output
            }

            ProofNode::RuleReference {
                rule_path,
                expansion,
                result,
                ..
            } => {
                let rule_key = rule_path.to_string();

                // Check if already expanded
                if expanded_rules.contains(&rule_key) {
                    // Just show the value with proper indentation
                    let value_str = match result {
                        lemma::OperationResult::Value(v) => v.to_string(),
                        lemma::OperationResult::Veto(_) => "<veto>".to_string(),
                    };
                    let mut output = String::new();
                    output.push_str(&format!("{}├─ {}?\n", prefix, rule_path));
                    output.push_str(&format!("{}│  └> {}\n", prefix, value_str));
                    output
                } else {
                    // Mark as expanded and show full expansion
                    expanded_rules.insert(rule_key);
                    let mut output = String::new();
                    output.push_str(&format!("{}├─ {}?\n", prefix, rule_path));
                    let expand_prefix = format!("{}│  ", prefix);
                    output.push_str(&self.format_proof_node(
                        expansion,
                        &expand_prefix,
                        expanded_rules,
                    ));
                    output
                }
            }

            ProofNode::Branches {
                matched,
                non_matched,
                ..
            } => {
                let mut output = String::new();

                // Show matched branch
                if let Some(condition) = &matched.condition {
                    output.push_str(&self.format_condition_node(condition, prefix, expanded_rules));
                    output.push_str(&self.format_result_expression(
                        &matched.result,
                        prefix,
                        expanded_rules,
                    ));
                } else {
                    output.push_str(&self.format_proof_node(
                        &matched.result,
                        prefix,
                        expanded_rules,
                    ));
                }

                // Show non-matched branches
                for branch in non_matched {
                    output.push_str(&self.format_non_matched_branch(branch, prefix));
                }

                output
            }

            ProofNode::Condition {
                original_expression,
                expression,
                ..
            } => {
                let mut output = String::new();
                output.push_str(&format!("{}{}\n", prefix, original_expression));
                output.push_str(&format!("{}├─ = {}\n", prefix, expression));
                output
            }

            ProofNode::Veto { message, .. } => {
                if let Some(msg) = message {
                    format!("{}└> Veto: {}\n", prefix, msg)
                } else {
                    format!("{}└> Veto\n", prefix)
                }
            }
        }
    }

    fn format_condition_node(
        &self,
        node: &lemma::proof::ProofNode,
        prefix: &str,
        expanded_rules: &mut HashSet<String>,
    ) -> String {
        use lemma::proof::ProofNode;

        match node {
            ProofNode::Computation {
                original_expression,
                expression,
                result,
                operands,
                ..
            } => {
                let mut output = String::new();
                output.push_str(&format!("{}{}\n", prefix, original_expression));

                // Expand rule references
                let rule_refs = Self::collect_rule_references(operands);
                for rule_ref in rule_refs {
                    output.push_str(&self.format_rule_reference_expansion(
                        rule_ref,
                        prefix,
                        expanded_rules,
                    ));
                }

                output.push_str(&format!("{}├─ = {}\n", prefix, expression));
                // Show the condition result (computed from the expression above)
                output.push_str(&format!("{}├─ = {}\n", prefix, result));
                output
            }
            ProofNode::Condition {
                original_expression,
                expression,
                result,
                operands,
                ..
            } => {
                let mut output = String::new();
                output.push_str(&format!("{}{}\n", prefix, original_expression));

                // Expand rule references
                let rule_refs = Self::collect_rule_references(operands);
                for rule_ref in rule_refs {
                    output.push_str(&self.format_rule_reference_expansion(
                        rule_ref,
                        prefix,
                        expanded_rules,
                    ));
                }

                output.push_str(&format!("{}├─ = {}\n", prefix, expression));
                // Show the condition result (computed from the expression above)
                output.push_str(&format!("{}├─ = {}\n", prefix, result));
                output
            }
            ProofNode::Value { value, source, .. } => {
                let condition_text = match source {
                    lemma::proof::ValueSource::Fact { fact_ref } => fact_ref.to_string(),
                    _ => value.to_string(),
                };
                format!("{}{}\n", prefix, condition_text)
            }
            _ => String::new(),
        }
    }

    fn format_result_expression(
        &self,
        node: &lemma::proof::ProofNode,
        prefix: &str,
        expanded_rules: &mut HashSet<String>,
    ) -> String {
        use lemma::proof::ProofNode;

        match node {
            ProofNode::Value { value, .. } => {
                format!("{}└> {}\n", prefix, value)
            }
            ProofNode::Veto { message, .. } => {
                if let Some(msg) = message {
                    format!("{}└> Veto: {}\n", prefix, msg)
                } else {
                    format!("{}└> Veto\n", prefix)
                }
            }
            ProofNode::Computation {
                original_expression,
                expression,
                result,
                operands,
                ..
            } => {
                let mut output = String::new();
                output.push_str(&format!("{}{}\n", prefix, original_expression));

                // Expand rule references
                let rule_refs = Self::collect_rule_references(operands);
                for rule_ref in rule_refs {
                    output.push_str(&self.format_rule_reference_expansion(
                        rule_ref,
                        prefix,
                        expanded_rules,
                    ));
                }

                output.push_str(&format!("{}├─ = {}\n", prefix, expression));
                output.push_str(&format!("{}└> = {}\n", prefix, result));
                output
            }
            _ => String::new(),
        }
    }

    fn format_non_matched_branch(
        &self,
        branch: &lemma::proof::NonMatchedBranch,
        prefix: &str,
    ) -> String {
        use lemma::proof::ProofNode;

        let mut output = String::new();
        let indent_prefix = format!("{}   ", prefix);

        // Format the condition
        match &*branch.condition {
            ProofNode::Computation {
                original_expression,
                expression,
                result,
                ..
            } => {
                output.push_str(&format!("{}×─ {}\n", prefix, original_expression));
                output.push_str(&format!("{}├─ = {}\n", indent_prefix, expression));

                // For veto branches, show the condition result as intermediate and then the veto
                if matches!(&*branch.result, ProofNode::Veto { .. }) {
                    output.push_str(&format!("{}├─ = {}\n", indent_prefix, result));
                    let result_str = self.format_result_display(&branch.result);
                    output.push_str(&format!("{}└─ {}\n", indent_prefix, result_str));
                } else {
                    // For non-veto branches, the condition result is the final result
                    output.push_str(&format!("{}└> = {}\n", indent_prefix, result));
                }
            }
            ProofNode::Condition {
                original_expression,
                expression,
                result,
                ..
            } => {
                output.push_str(&format!("{}×─ {}\n", prefix, original_expression));
                output.push_str(&format!("{}├─ = {}\n", indent_prefix, expression));

                // For veto branches, show the condition result as intermediate and then the veto
                if matches!(&*branch.result, ProofNode::Veto { .. }) {
                    output.push_str(&format!("{}├─ = {}\n", indent_prefix, result));
                    let result_str = self.format_result_display(&branch.result);
                    output.push_str(&format!("{}└─ {}\n", indent_prefix, result_str));
                } else {
                    // For non-veto branches, the condition result is the final result
                    output.push_str(&format!("{}└> = {}\n", indent_prefix, result));
                }
            }
            ProofNode::Value { value, source, .. } => {
                let condition_text = match source {
                    lemma::proof::ValueSource::Fact { fact_ref } => fact_ref.to_string(),
                    _ => value.to_string(),
                };
                output.push_str(&format!("{}×─ {}\n", prefix, condition_text));

                // For veto branches, show the value as intermediate and then the veto
                if matches!(&*branch.result, ProofNode::Veto { .. }) {
                    output.push_str(&format!("{}├─ {}\n", indent_prefix, value));
                    let result_str = self.format_result_display(&branch.result);
                    output.push_str(&format!("{}└─ {}\n", indent_prefix, result_str));
                } else {
                    // For non-veto branches, the value is the final result (not computed, so no =)
                    output.push_str(&format!("{}└> {}\n", indent_prefix, value));
                }
            }
            _ => {}
        }

        output
    }

    /// Helper to format the result part for displaying in "then" clauses
    fn format_result_display(&self, node: &lemma::proof::ProofNode) -> String {
        use lemma::proof::ProofNode;

        match node {
            ProofNode::Veto { message, .. } => {
                if let Some(msg) = message {
                    format!("Veto: {}", msg)
                } else {
                    "Veto".to_string()
                }
            }
            ProofNode::Value { value, .. } => value.to_string(),
            ProofNode::Computation {
                original_expression,
                ..
            } => original_expression.clone(),
            _ => "<expression>".to_string(),
        }
    }

    fn collect_rule_references(
        operands: &[lemma::proof::ProofNode],
    ) -> Vec<&lemma::proof::ProofNode> {
        let mut refs = Vec::new();
        for operand in operands {
            if matches!(operand, lemma::proof::ProofNode::RuleReference { .. }) {
                refs.push(operand);
            } else if let lemma::proof::ProofNode::Computation { operands, .. } = operand {
                refs.extend(Self::collect_rule_references(operands));
            }
        }
        refs
    }

    fn format_rule_reference_expansion(
        &self,
        node: &lemma::proof::ProofNode,
        prefix: &str,
        expanded_rules: &mut HashSet<String>,
    ) -> String {
        if let lemma::proof::ProofNode::RuleReference {
            rule_path,
            expansion,
            result,
            ..
        } = node
        {
            let rule_key = rule_path.to_string();

            // Check if already expanded
            if expanded_rules.contains(&rule_key) {
                // Just show the value with proper indentation
                let value_str = match result {
                    lemma::OperationResult::Value(v) => v.to_string(),
                    lemma::OperationResult::Veto(_) => "<veto>".to_string(),
                };
                let mut output = String::new();
                output.push_str(&format!("{}├─ {}?\n", prefix, rule_path));
                output.push_str(&format!("{}│  └> {}\n", prefix, value_str));
                output
            } else {
                // Mark as expanded and show full expansion
                expanded_rules.insert(rule_key);
                let mut output = String::new();
                output.push_str(&format!("{}├─ {}?\n", prefix, rule_path));
                let expand_prefix = format!("{}│  ", prefix);
                output.push_str(&self.format_proof_node(expansion, &expand_prefix, expanded_rules));
                output
            }
        } else {
            String::new()
        }
    }

    pub fn format_document_inspection(
        &self,
        doc: &lemma::LemmaDoc,
        facts: &[&lemma::LemmaFact],
        rules: &[&lemma::LemmaRule],
    ) -> String {
        let mut output = String::new();
        output.push_str(&format!("Document: {}\n\n", doc.name));
        output.push_str("facts:\n");
        for fact in facts {
            if let lemma::FactType::Local(name) = &fact.fact_type {
                output.push_str(&format!("  - {}\n", name));
            }
        }
        output.push_str("\nrules:\n");
        for rule in rules {
            output.push_str(&format!("  - {}\n", rule.name));
        }
        output
    }

    pub fn format_workspace_summary(
        &self,
        file_count: usize,
        doc_count: usize,
        doc_stats: &[(String, usize, usize)],
    ) -> String {
        let mut output = String::new();
        let file_word = if file_count == 1 { "file" } else { "files" };
        let doc_word = if doc_count == 1 {
            "document"
        } else {
            "documents"
        };
        output.push_str(&format!(
            "Found {} {} in {} {}\n\n",
            doc_count, doc_word, file_count, file_word
        ));
        let mut table = Table::new();
        table.load_preset(UTF8_FULL);
        table.set_header(vec!["Document", "Facts", "Rules"]);
        for (doc_name, facts, rules) in doc_stats {
            table.add_row(vec![
                doc_name.as_str(),
                &facts.to_string(),
                &rules.to_string(),
            ]);
        }
        output.push_str(&table.to_string());
        output
    }

    pub fn format_inversion_result(
        &self,
        solutions: &[std::collections::HashMap<lemma::FactReference, lemma::Domain>],
    ) -> String {
        let mut output = String::new();
        if solutions.is_empty() {
            output.push_str("No solutions found.\n");
        } else {
            output.push_str(&format!("Found {} solution(s):\n\n", solutions.len()));
            for (i, solution) in solutions.iter().enumerate() {
                output.push_str(&format!("Solution {}:\n", i + 1));
                for (fact_ref, domain) in solution {
                    output.push_str(&format!("  {} = {:?}\n", fact_ref, domain));
                }
                output.push('\n');
            }
        }
        output
    }
}
