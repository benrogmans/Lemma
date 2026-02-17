use lemma::evaluation::proof::{NonMatchedBranch, ProofNode, ValueSource};
use lemma::planning::semantics::{FactPath, FactValue, TypeSpecification, ValueKind};
use lemma::{DocumentSchema, ExecutionPlan, LiteralValue, OperationResult, Response, RuleResult};
use std::collections::HashSet;
use super_table::{presets, Cell, CellAlignment, Table};

struct Row {
    left: String,
    unit: String,
    value: String,
}

#[derive(Clone, Copy)]
enum Connector {
    Branch,
    Last,
}

struct RenderContext<'a> {
    rows: &'a mut Vec<Row>,
    expanded: &'a mut HashSet<String>,
    indent: &'a str,
}

pub struct Formatter;

impl Default for Formatter {
    fn default() -> Self {
        Self
    }
}

impl Formatter {
    /// Format evaluation response. When `explain` is false: one line for a single rule, or one table
    /// for multiple rules. When true: facts tree and full proof trees per rule.
    pub fn format_response(&self, response: &Response, explain: bool) -> String {
        if response.results.is_empty() {
            return String::new();
        }

        if explain {
            return self.format_response_explain(response);
        }

        if response.results.len() == 1 {
            let result = response.results.values().next().unwrap();
            let (unit, value) = self.split_result(&result.result);
            let line = if unit.is_empty() {
                value
            } else {
                format!("{} {}", value, unit)
            };
            return format!("{}\n", line);
        }

        let mut table = Table::new();
        table.load_preset(presets::UTF8_FULL);
        table.set_style(super_table::TableComponent::MiddleIntersections, '┼');
        table.set_style(super_table::TableComponent::HorizontalLines, '─');
        for result in response.results.values() {
            let (unit, value) = self.split_result(&result.result);
            let value_cell = if unit.is_empty() {
                value
            } else {
                format!("{} {}", value, unit)
            };
            table.add_row(vec![
                Cell::new(&result.rule.name).set_alignment(CellAlignment::Left),
                Cell::new(&value_cell).set_alignment(CellAlignment::Left),
            ]);
        }
        format!("{}\n", table)
    }

    fn format_response_explain(&self, response: &Response) -> String {
        let mut output = String::new();
        if !response.facts.is_empty() {
            output.push_str("Facts\n");
            output.push_str(&self.format_facts_tree(&response.facts, &response.doc_name));
            output.push('\n');
        }
        if !response.results.is_empty() {
            output.push_str("Rules\n");
            for result in response.results.values() {
                output.push_str(&self.format_rule_result(result));
                output.push('\n');
            }
        }
        output
    }

    pub fn format_document_inspection(&self, plan: &ExecutionPlan) -> String {
        let local_fact_paths: Vec<&FactPath> = plan
            .facts
            .keys()
            .filter(|p| p.segments.is_empty())
            .collect();

        let mut table = Table::new();
        table.load_preset(presets::UTF8_FULL);
        table.set_style(super_table::TableComponent::MiddleIntersections, '┼');
        table.set_style(super_table::TableComponent::HorizontalLines, '─');

        table.add_row(vec![
            Cell::new(&plan.doc_name).set_alignment(CellAlignment::Left)
        ]);

        let mut content_lines = Vec::new();

        if !local_fact_paths.is_empty() {
            content_lines.push("facts".to_string());
            for (i, path) in local_fact_paths.iter().enumerate() {
                let prefix = if i == local_fact_paths.len() - 1 {
                    "└─"
                } else {
                    "├─"
                };
                content_lines.push(format!("{} {}", prefix, path.fact));
            }
        }

        if !plan.rules.is_empty() {
            content_lines.push("rules".to_string());
            for (i, rule) in plan.rules.iter().enumerate() {
                let prefix = if i == plan.rules.len() - 1 {
                    "└─"
                } else {
                    "├─"
                };
                content_lines.push(format!("{} {}", prefix, rule.name));
            }
        }

        table.add_row(vec![
            Cell::new(content_lines.join("\n")).set_alignment(CellAlignment::Left)
        ]);

        format!("{}\n", table)
    }

    pub fn format_workspace_summary(
        &self,
        file_count: usize,
        schemas: &[DocumentSchema],
    ) -> String {
        let mut output = String::new();
        let doc_count = schemas.len();
        let file_word = if file_count == 1 { "file" } else { "files" };
        let doc_word = if doc_count == 1 {
            "document"
        } else {
            "documents"
        };
        output.push_str(&format!(
            "Found {} {} in {} {}\n",
            doc_count, doc_word, file_count, file_word
        ));

        for schema in schemas {
            output.push('\n');

            let mut table = Table::new();
            table.load_preset(presets::UTF8_FULL);

            table.set_style(super_table::TableComponent::HeaderLines, '─');
            table.set_style(super_table::TableComponent::LeftHeaderIntersection, '├');
            table.set_style(super_table::TableComponent::MiddleHeaderIntersections, '┼');
            table.set_style(super_table::TableComponent::RightHeaderIntersection, '┤');
            table.set_style(super_table::TableComponent::MiddleIntersections, '┼');
            table.set_style(super_table::TableComponent::HorizontalLines, '─');

            table.set_header(vec![
                Cell::new(&schema.doc).set_alignment(CellAlignment::Left),
                Cell::new(""),
                Cell::new(""),
            ]);

            if schema.facts.is_empty() && schema.rules.is_empty() {
                table.add_row(vec![
                    Cell::new("(no facts or rules)").set_alignment(CellAlignment::Left),
                    Cell::new(""),
                    Cell::new(""),
                ]);
                output.push_str(&table.to_string());
                continue;
            }

            let mut col_name = Vec::new();
            let mut col_type = Vec::new();
            let mut col_default = Vec::new();

            if !schema.facts.is_empty() {
                col_name.push("Facts".to_string());
                col_type.push(String::new());
                col_default.push(String::new());
                for (name, (lemma_type, default)) in &schema.facts {
                    col_name.push(format!("  {}", name));
                    col_type.push(lemma_type.name());
                    col_default.push(default.as_ref().map(|v| v.to_string()).unwrap_or_default());
                }
            }

            if !schema.facts.is_empty() && !schema.rules.is_empty() {
                col_name.push(String::new());
                col_type.push(String::new());
                col_default.push(String::new());
            }

            if !schema.rules.is_empty() {
                col_name.push("Rules".to_string());
                col_type.push(String::new());
                col_default.push(String::new());
                for (name, rule_type) in &schema.rules {
                    col_name.push(format!("  {}", name));
                    col_type.push(rule_type.name());
                    col_default.push(String::new());
                }
            }

            table.add_row(vec![
                Cell::new(col_name.join("\n")).set_alignment(CellAlignment::Left),
                Cell::new(col_type.join("\n")).set_alignment(CellAlignment::Left),
                Cell::new(col_default.join("\n")).set_alignment(CellAlignment::Left),
            ]);

            output.push_str(&table.to_string());
        }

        output
    }

    fn format_facts_tree(&self, facts_groups: &[lemma::Facts], doc_name: &str) -> String {
        let mut output = String::new();

        for group in facts_groups {
            if group.facts.is_empty() && group.referenced_docs.is_empty() {
                continue;
            }

            let mut table = Table::new();
            table.load_preset(presets::UTF8_FULL);
            table.set_style(super_table::TableComponent::MiddleIntersections, '┼');
            table.set_style(super_table::TableComponent::HorizontalLines, '─');

            table.add_row(vec![
                Cell::new(doc_name.to_string()).set_alignment(CellAlignment::Left),
                Cell::new("").set_alignment(CellAlignment::Left),
                Cell::new("").set_alignment(CellAlignment::Left),
            ]);

            let (name_content, type_content, value_content) =
                if let Some(doc_ref) = &group.document_reference {
                    let mut name_lines = vec![group.referencing_fact_name.clone()];
                    let mut type_lines = vec!["doc".to_string()];
                    let mut value_lines = vec![format!("{}", doc_ref)];

                    let (nested_name, nested_type, nested_value) =
                        self.build_facts_content_for_referenced_doc(group);
                    if !nested_name.is_empty() {
                        name_lines.push(nested_name);
                        type_lines.push(nested_type);
                        value_lines.push(nested_value);
                    }

                    (
                        name_lines.join("\n"),
                        type_lines.join("\n"),
                        value_lines.join("\n"),
                    )
                } else {
                    self.build_facts_content(group, "")
                };

            table.add_row(vec![
                Cell::new(name_content).set_alignment(CellAlignment::Left),
                Cell::new(type_content).set_alignment(CellAlignment::Left),
                Cell::new(value_content).set_alignment(CellAlignment::Left),
            ]);

            output.push_str(&table.to_string());
            output.push('\n');
        }

        output
    }

    fn build_facts_content_for_referenced_doc(
        &self,
        group: &lemma::Facts,
    ) -> (String, String, String) {
        let mut name_lines = Vec::new();
        let mut type_lines = Vec::new();
        let mut value_lines = Vec::new();

        for fact in &group.facts {
            let value_str = match &fact.value {
                FactValue::Literal(lit) => self.format_literal(lit),
                FactValue::DocumentReference(doc_name) => format!("doc {}", doc_name),
                FactValue::TypeDeclaration { .. } => String::new(),
            };
            name_lines.push(fact.path.to_string());
            type_lines.push(Self::fact_type_str(&fact.value));
            value_lines.push(value_str);
        }

        (
            name_lines.join("\n"),
            type_lines.join("\n"),
            value_lines.join("\n"),
        )
    }

    fn build_facts_content(&self, group: &lemma::Facts, prefix: &str) -> (String, String, String) {
        let mut name_lines = Vec::new();
        let mut type_lines = Vec::new();
        let mut value_lines = Vec::new();

        let next_prefix = if prefix.is_empty() {
            String::new()
        } else {
            format!("{}  ", prefix)
        };

        for child_group in &group.referenced_docs {
            let doc_name_str = child_group
                .document_reference
                .as_ref()
                .map(|d| format!("doc {}", d))
                .unwrap_or_default();

            name_lines.push(child_group.referencing_fact_name.clone());
            type_lines.push("doc".to_string());
            value_lines.push(doc_name_str);

            let (child_name, child_type, child_value) = self.build_facts_content(child_group, "  ");
            if !child_name.is_empty() {
                name_lines.push(child_name);
                type_lines.push(child_type);
                value_lines.push(child_value);
            }
        }

        for fact in &group.facts {
            let value_str = match &fact.value {
                FactValue::Literal(lit) => self.format_literal(lit),
                FactValue::DocumentReference(doc_name) => format!("doc {}", doc_name),
                FactValue::TypeDeclaration { .. } => String::new(),
            };
            name_lines.push(format!("{}{}", next_prefix, fact.path));
            type_lines.push(Self::fact_type_str(&fact.value));
            value_lines.push(value_str);
        }

        (
            name_lines.join("\n"),
            type_lines.join("\n"),
            value_lines.join("\n"),
        )
    }

    fn fact_type_str(value: &FactValue) -> String {
        match value {
            FactValue::Literal(lit) => lit.lemma_type.name(),
            FactValue::TypeDeclaration { resolved_type } => resolved_type.name(),
            FactValue::DocumentReference(doc_name) => format!("doc {}", doc_name),
        }
    }

    fn format_literal(&self, lit: &LiteralValue) -> String {
        match &lit.value {
            ValueKind::Text(s) => s.clone(),
            _ => lit.to_string(),
        }
    }

    fn format_rule_result(&self, result: &RuleResult) -> String {
        let mut rows: Vec<Row> = Vec::new();
        let mut expanded: HashSet<String> = HashSet::new();

        if let Some(proof) = &result.proof {
            self.render_node(&proof.tree, "", &mut rows, &mut expanded);
        }

        let mut table = Table::new();
        table.load_preset(presets::UTF8_FULL);
        table.set_style(super_table::TableComponent::MiddleIntersections, '┼');
        table.set_style(super_table::TableComponent::HorizontalLines, '─');

        let (unit, value) = self.split_result(&result.result);
        table.add_row(vec![
            Cell::new(&result.rule.name).set_alignment(CellAlignment::Left),
            Cell::new(&value).set_alignment(CellAlignment::Right),
            Cell::new(&unit).set_alignment(CellAlignment::Left),
        ]);

        if !rows.is_empty() {
            let left_content = rows
                .iter()
                .map(|r| r.left.as_str())
                .collect::<Vec<_>>()
                .join("\n");

            let value_content = rows
                .iter()
                .map(|r| r.value.as_str())
                .collect::<Vec<_>>()
                .join("\n");

            let unit_content = rows
                .iter()
                .map(|r| r.unit.as_str())
                .collect::<Vec<_>>()
                .join("\n");

            table.add_row(vec![
                Cell::new(left_content).set_alignment(CellAlignment::Left),
                Cell::new(value_content).set_alignment(CellAlignment::Right),
                Cell::new(unit_content).set_alignment(CellAlignment::Left),
            ]);
        }

        let source = &result.rule.source_location;
        let location = format!(
            "Source: {}:{}:{}",
            source.attribute, source.span.line, source.span.col
        );
        table.add_row(vec![Cell::new(self.gray(&location))
            .set_alignment(CellAlignment::Left)
            .set_colspan(3)]);

        if let Some(last_column) = table.column_mut(2) {
            use super_table::ColumnConstraint;
            last_column.set_constraint(ColumnConstraint::UpperBoundary(super_table::Width::Fixed(
                10,
            )));
        }

        table.to_string()
    }

    fn render_node(
        &self,
        node: &ProofNode,
        indent: &str,
        rows: &mut Vec<Row>,
        expanded: &mut HashSet<String>,
    ) {
        let mut ctx = RenderContext {
            rows,
            expanded,
            indent,
        };
        match node {
            ProofNode::Value { value, source, .. } => {
                self.render_value(value, source, &mut ctx);
            }
            ProofNode::RuleReference {
                rule_path,
                result,
                expansion,
                ..
            } => {
                self.render_rule_reference(rule_path, result, expansion, Connector::Last, &mut ctx);
            }
            ProofNode::Computation {
                expression,
                original_expression,
                operands,
                ..
            } => {
                self.render_computation(expression, original_expression, operands, &mut ctx);
            }
            ProofNode::Branches {
                matched,
                non_matched,
                ..
            } => {
                self.render_branches(matched, non_matched, &mut ctx);
            }
            ProofNode::Condition {
                expression,
                original_expression,
                result,
                operands,
                ..
            } => {
                self.render_condition(expression, original_expression, *result, operands, &mut ctx);
            }
            ProofNode::Veto { message, .. } => {
                self.render_veto(message, &mut ctx);
            }
        }
    }

    fn render_node_with_connector(
        &self,
        node: &ProofNode,
        indent: &str,
        connector: Connector,
        rows: &mut Vec<Row>,
        expanded: &mut HashSet<String>,
    ) {
        let mut ctx = RenderContext {
            rows,
            expanded,
            indent,
        };
        match node {
            ProofNode::Value { value, source, .. } => {
                let display = match source {
                    ValueSource::Fact { fact_ref } => fact_ref.to_string(),
                    ValueSource::Literal | ValueSource::Computed => value.to_string(),
                };
                ctx.rows.push(Row {
                    left: format!(
                        "{}{} {}",
                        ctx.indent,
                        self.connector_str(connector),
                        display
                    ),
                    unit: String::new(),
                    value: String::new(),
                });
            }
            ProofNode::RuleReference {
                rule_path,
                result,
                expansion,
                ..
            } => {
                self.render_rule_reference(rule_path, result, expansion, connector, &mut ctx);
            }
            _ => {
                self.render_node(node, indent, rows, expanded);
            }
        }
    }

    fn render_value(&self, value: &LiteralValue, source: &ValueSource, ctx: &mut RenderContext) {
        let display = match source {
            ValueSource::Fact { fact_ref } => fact_ref.to_string(),
            ValueSource::Literal | ValueSource::Computed => value.to_string(),
        };
        ctx.rows.push(Row {
            left: format!("{}└─ {}", ctx.indent, display),
            unit: String::new(),
            value: String::new(),
        });
    }

    fn render_rule_reference(
        &self,
        rule_path: &lemma::RulePath,
        result: &OperationResult,
        expansion: &ProofNode,
        connector: Connector,
        ctx: &mut RenderContext,
    ) {
        let rule_key = rule_path.to_string();
        let (unit, value) = self.split_result(result);
        ctx.rows.push(Row {
            left: format!(
                "{}{} {}",
                ctx.indent,
                self.connector_str(connector),
                rule_path
            ),
            unit,
            value,
        });

        if ctx.expanded.insert(rule_key) {
            let child_indent = self.child_indent(ctx.indent, connector);
            self.render_node(expansion, &child_indent, ctx.rows, ctx.expanded);
        }
    }

    fn render_computation(
        &self,
        expression: &str,
        original_expression: &str,
        operands: &[ProofNode],
        ctx: &mut RenderContext,
    ) {
        ctx.rows.push(Row {
            left: format!("{}├─ {}", ctx.indent, expression),
            unit: String::new(),
            value: String::new(),
        });
        ctx.rows.push(Row {
            left: format!("{}└─ {}", ctx.indent, original_expression),
            unit: String::new(),
            value: String::new(),
        });

        let child_indent = format!("{}   ", ctx.indent);
        let rule_children: Vec<&ProofNode> = operands
            .iter()
            .filter(|op| matches!(op, ProofNode::RuleReference { .. }))
            .collect();

        let len = rule_children.len();
        for (i, child) in rule_children.iter().enumerate() {
            let connector = if i == len - 1 {
                Connector::Last
            } else {
                Connector::Branch
            };
            self.render_node_with_connector(
                child,
                &child_indent,
                connector,
                ctx.rows,
                ctx.expanded,
            );
        }
    }

    fn render_branches(
        &self,
        matched: &lemma::evaluation::proof::Branch,
        non_matched: &[NonMatchedBranch],
        ctx: &mut RenderContext,
    ) {
        enum BranchItem<'a> {
            Matched(&'a lemma::evaluation::proof::Branch),
            NonMatched(&'a NonMatchedBranch),
        }

        let mut all_branches: Vec<((bool, usize), BranchItem)> = Vec::new();

        let matched_key = match matched.clause_index {
            None => (false, 0),
            Some(idx) => (true, idx),
        };
        all_branches.push((matched_key, BranchItem::Matched(matched)));

        for branch in non_matched {
            let key = match branch.clause_index {
                None => (false, 0),
                Some(idx) => (true, idx),
            };
            all_branches.push((key, BranchItem::NonMatched(branch)));
        }

        all_branches.sort_by_key(|((is_some, idx), _)| (*is_some, *idx));

        for (_, branch_item) in all_branches {
            match branch_item {
                BranchItem::Matched(branch) => {
                    let has_condition = branch.condition.is_some();

                    if let Some(condition) = &branch.condition {
                        ctx.rows.push(Row {
                            left: format!(
                                "{}✓ {}",
                                ctx.indent,
                                self.extract_condition_text(condition)
                            ),
                            unit: String::new(),
                            value: String::new(),
                        });
                    }

                    if !matches!(&*branch.result, ProofNode::Value { .. }) {
                        let result_indent = if has_condition {
                            format!("{}   ", ctx.indent)
                        } else {
                            ctx.indent.to_string()
                        };
                        self.render_node(&branch.result, &result_indent, ctx.rows, ctx.expanded);
                    }
                }
                BranchItem::NonMatched(branch) => {
                    let condition_result =
                        Self::extract_condition_result(&branch.condition).unwrap_or_default();
                    ctx.rows.push(Row {
                        left: format!(
                            "{}{}",
                            ctx.indent,
                            self.gray(&format!(
                                "✗ {}",
                                self.extract_condition_text(&branch.condition)
                            ))
                        ),
                        unit: String::new(),
                        value: condition_result,
                    });
                    let condition_indent = format!("{}  ", ctx.indent);
                    self.render_condition_operands_only(
                        &branch.condition,
                        &condition_indent,
                        ctx.rows,
                        ctx.expanded,
                    );
                }
            }
        }
    }

    /// Renders only the operands of a condition node (Computation or Condition), skipping the
    /// redundant expression lines. Used for non-matched branch conditions so we show
    /// condition text once with the result, then just the rule-reference operands and their expansion.
    fn render_condition_operands_only(
        &self,
        node: &ProofNode,
        indent: &str,
        rows: &mut Vec<Row>,
        expanded: &mut HashSet<String>,
    ) {
        let operands: &[ProofNode] = match node {
            ProofNode::Computation { operands, .. } => operands.as_ref(),
            ProofNode::Condition { operands, .. } => operands.as_ref(),
            _ => return,
        };
        let rule_children: Vec<&ProofNode> = operands
            .iter()
            .filter(|op| matches!(op, ProofNode::RuleReference { .. }))
            .collect();
        let len = rule_children.len();
        for (i, child) in rule_children.iter().enumerate() {
            let connector = if i == len - 1 {
                Connector::Last
            } else {
                Connector::Branch
            };
            self.render_node_with_connector(child, indent, connector, rows, expanded);
        }
    }

    fn render_condition(
        &self,
        expression: &str,
        original_expression: &str,
        result: bool,
        operands: &[ProofNode],
        ctx: &mut RenderContext,
    ) {
        ctx.rows.push(Row {
            left: format!("{}├─ {}", ctx.indent, expression),
            unit: String::new(),
            value: if result {
                "true".to_string()
            } else {
                "false".to_string()
            },
        });
        ctx.rows.push(Row {
            left: format!("{}└─ {}", ctx.indent, original_expression),
            unit: String::new(),
            value: String::new(),
        });

        let child_indent = format!("{}   ", ctx.indent);
        let rule_children: Vec<&ProofNode> = operands
            .iter()
            .filter(|op| matches!(op, ProofNode::RuleReference { .. }))
            .collect();

        let len = rule_children.len();
        for (i, child) in rule_children.iter().enumerate() {
            let connector = if i == len - 1 {
                Connector::Last
            } else {
                Connector::Branch
            };
            self.render_node_with_connector(
                child,
                &child_indent,
                connector,
                ctx.rows,
                ctx.expanded,
            );
        }
    }

    fn render_veto(&self, message: &Option<String>, ctx: &mut RenderContext) {
        let msg = message.as_deref().unwrap_or("");
        ctx.rows.push(Row {
            left: format!("{}└─ veto", ctx.indent),
            unit: String::new(),
            value: format!("Veto: {}", msg),
        });
    }

    fn connector_str(&self, connector: Connector) -> &'static str {
        match connector {
            Connector::Branch => "├─",
            Connector::Last => "└─",
        }
    }

    fn child_indent(&self, parent_indent: &str, connector: Connector) -> String {
        match connector {
            Connector::Branch => format!("{}│  ", parent_indent),
            Connector::Last => format!("{}   ", parent_indent),
        }
    }

    fn split_result(&self, result: &OperationResult) -> (String, String) {
        match result {
            OperationResult::Value(v) => self.split_literal(v),
            OperationResult::Veto(msg) => (
                String::new(),
                msg.as_ref()
                    .map(|m| format!("Veto: {}", m))
                    .unwrap_or_else(|| "Veto".to_string()),
            ),
        }
    }

    fn split_literal(&self, lit: &LiteralValue) -> (String, String) {
        match &lit.value {
            ValueKind::Number(n) => {
                let decimals_opt = lit.lemma_type.decimal_places();
                (String::new(), format_decimal(n, decimals_opt))
            }
            ValueKind::Scale(n, unit) => {
                let decimals_opt = lit.lemma_type.decimal_places();
                (unit.clone(), format_decimal(n, decimals_opt))
            }
            ValueKind::Ratio(r, unit_opt) => {
                let decimals_opt = lit.lemma_type.decimal_places();
                match unit_opt.as_deref() {
                    Some(unit_name) => {
                        let display_value = if let TypeSpecification::Ratio { units, .. } =
                            &lit.lemma_type.specifications
                        {
                            if let Ok(unit) = units.get(unit_name) {
                                *r * unit.value
                            } else {
                                *r
                            }
                        } else {
                            *r
                        };
                        let display_unit = if unit_name == "percent" {
                            "%".to_string()
                        } else {
                            unit_name.to_string()
                        };
                        (display_unit, format_decimal(&display_value, decimals_opt))
                    }
                    None => (String::new(), format_decimal(r, decimals_opt)),
                }
            }
            ValueKind::Text(s) => (String::new(), s.clone()),
            ValueKind::Boolean(b) => (String::new(), b.to_string()),
            ValueKind::Date(d) => (String::new(), d.to_string()),
            ValueKind::Time(t) => (String::new(), t.to_string()),
            ValueKind::Duration(value, unit) => (unit.to_string(), format_decimal(value, None)),
        }
    }

    /// Returns the condition result as "true"/"false" for the value column, if the node is a boolean condition.
    fn extract_condition_result(node: &ProofNode) -> Option<String> {
        match node {
            ProofNode::Computation { result, .. } => match &result.value {
                ValueKind::Boolean(b) => Some(b.to_string()),
                _ => None,
            },
            ProofNode::Condition { result, .. } => Some(result.to_string()),
            _ => None,
        }
    }

    fn extract_condition_text(&self, node: &ProofNode) -> String {
        match node {
            ProofNode::Computation {
                original_expression,
                ..
            } => original_expression.clone(),
            ProofNode::Condition {
                original_expression,
                ..
            } => original_expression.clone(),
            ProofNode::Value { value, source, .. } => match source {
                ValueSource::Fact { fact_ref } => fact_ref.to_string(),
                ValueSource::Literal | ValueSource::Computed => value.to_string(),
            },
            ProofNode::RuleReference { rule_path, .. } => rule_path.to_string(),
            ProofNode::Branches { .. } => "<branches>".to_string(),
            ProofNode::Veto { message, .. } => {
                message.clone().unwrap_or_else(|| "veto".to_string())
            }
        }
    }

    fn gray(&self, text: &str) -> String {
        format!("\x1b[5;90m{}\x1b[0m", text)
    }
}

fn format_decimal(d: &rust_decimal::Decimal, decimals: Option<u8>) -> String {
    match decimals {
        Some(decimals) => {
            // Fixed-decimal formatting, preserving trailing zeros.
            let rounded = d.round_dp(decimals as u32);
            let mut s = rounded.to_string();
            if decimals == 0 {
                if let Some(dot) = s.find('.') {
                    s.truncate(dot);
                }
                return s;
            }
            if let Some(dot_pos) = s.find('.') {
                let current_decimals = s.len() - dot_pos - 1;
                if current_decimals < decimals as usize {
                    s.push_str(&"0".repeat(decimals as usize - current_decimals));
                }
            } else {
                s.push('.');
                s.push_str(&"0".repeat(decimals as usize));
            }
            s
        }
        None => {
            // No decimals specified: do not force rounding; remove trailing zeros.
            let normalized = d.normalize();
            if normalized.fract().is_zero() {
                normalized.trunc().to_string()
            } else {
                normalized.to_string()
            }
        }
    }
}
