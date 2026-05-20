use lemma::evaluation::explanation::{
    ConversionExplanationStep, ExplanationNode, NonMatchedBranch, ValueSource,
};
use lemma::evaluation::operations::ComputationKind;
use lemma::planning::semantics::{DataPath, DataValue, ValueKind};
use lemma::{
    commit_rational_to_decimal, rational_to_display_str, ExecutionPlan, LiteralValue,
    OperationResult, RationalInteger, Response, RuleResult, SpecSchema,
};
use std::collections::HashSet;
use super_table::{presets, Cell, CellAlignment, Table};

#[derive(Clone, Copy)]
enum Connector {
    Branch,
    Last,
}

struct RenderContext<'a> {
    rows: &'a mut Vec<String>,
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
    /// for multiple rules. When true: data tree and full explanation trees per rule.
    pub fn format_response(&self, response: &Response, explain: bool) -> String {
        if response.results.is_empty() {
            return String::new();
        }

        if explain {
            return self.format_response_explain(response);
        }

        if response.results.len() == 1 {
            let result = response
                .results
                .values()
                .next()
                .expect("BUG: len==1 but no values");
            return format!("{}\n", self.format_result_inline(&result.result));
        }

        let mut table = Table::new();
        table.load_preset(presets::UTF8_FULL);
        table.set_style(super_table::TableComponent::MiddleIntersections, '┼');
        table.set_style(super_table::TableComponent::HorizontalLines, '─');
        for result in response.results.values() {
            table.add_row(vec![
                Cell::new(&result.rule.name).set_alignment(CellAlignment::Left),
                Cell::new(self.format_result_inline(&result.result))
                    .set_alignment(CellAlignment::Left),
            ]);
        }
        format!("{}\n", table)
    }

    fn format_response_explain(&self, response: &Response) -> String {
        let mut output = String::new();
        if !response.data.is_empty() {
            output.push_str("Data\n");
            output.push_str(&self.format_data_tree(&response.data, &response.spec_name));
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

    pub fn format_spec_inspection(&self, plan: &ExecutionPlan) -> String {
        let local_data_paths: Vec<&DataPath> =
            plan.data.keys().filter(|p| p.segments.is_empty()).collect();

        let mut table = Table::new();
        table.load_preset(presets::UTF8_FULL);
        table.set_style(super_table::TableComponent::MiddleIntersections, '┼');
        table.set_style(super_table::TableComponent::HorizontalLines, '─');

        table.add_row(vec![
            Cell::new(&plan.spec_name).set_alignment(CellAlignment::Left)
        ]);

        let mut content_lines = Vec::new();

        if !local_data_paths.is_empty() {
            content_lines.push("data".to_string());
            for (i, path) in local_data_paths.iter().enumerate() {
                let prefix = if i == local_data_paths.len() - 1 {
                    "└─"
                } else {
                    "├─"
                };
                content_lines.push(format!("{} {}", prefix, path.data));
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

    pub fn format_workspace_summary(&self, source_count: usize, schemas: &[SpecSchema]) -> String {
        let mut output = String::new();
        let spec_count = schemas.len();
        let source_word = if source_count == 1 {
            "source"
        } else {
            "sources"
        };
        let spec_word = if spec_count == 1 { "spec" } else { "specs" };
        output.push_str(&format!(
            "Found {} {} in {} {}\n",
            spec_count, spec_word, source_count, source_word
        ));
        output.push_str(&self.format_spec_schema_tables(schemas));
        output
    }

    /// Tables only (no preamble). Used when listing a single repository; context is the header above.
    pub fn format_spec_schema_tables(&self, schemas: &[SpecSchema]) -> String {
        let mut output = String::new();
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
                Cell::new(&schema.spec).set_alignment(CellAlignment::Left),
                Cell::new(""),
                Cell::new(""),
            ]);

            if schema.data.is_empty() && schema.rules.is_empty() {
                table.add_row(vec![
                    Cell::new("(no data or rules)").set_alignment(CellAlignment::Left),
                    Cell::new(""),
                    Cell::new(""),
                ]);
                output.push_str(&table.to_string());
                continue;
            }

            let mut col_name = Vec::new();
            let mut col_type = Vec::new();
            let mut col_default = Vec::new();

            if !schema.data.is_empty() {
                col_name.push("Data".to_string());
                col_type.push(String::new());
                col_default.push(String::new());
                for (name, entry) in &schema.data {
                    col_name.push(format!("  {}", name));
                    col_type.push(entry.lemma_type.name());
                    col_default.push(match (&entry.bound_value, &entry.default) {
                        (Some(b), Some(d)) => format!("{}, default {}", b, d),
                        (Some(b), None) => b.to_string(),
                        (None, Some(d)) => d.to_string(),
                        (None, None) => String::new(),
                    });
                }
            }

            if !schema.data.is_empty() && !schema.rules.is_empty() {
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

    pub fn format_repositories_summary(&self, repos: &[(String, usize)]) -> String {
        if repos.is_empty() {
            return String::new();
        }
        let mut s = String::from("\nRepositories:\n");
        for (name, count) in repos {
            let word = if *count == 1 { "spec" } else { "specs" };
            s.push_str(&format!("  {} ({} {})\n", name, count, word));
        }
        s
    }

    fn format_data_tree(&self, data_groups: &[lemma::DataGroup], spec_name: &str) -> String {
        let mut output = String::new();

        for group in data_groups {
            if group.data.is_empty() {
                continue;
            }

            let mut table = Table::new();
            table.load_preset(presets::UTF8_FULL);
            table.set_style(super_table::TableComponent::MiddleIntersections, '┼');
            table.set_style(super_table::TableComponent::HorizontalLines, '─');

            table.add_row(vec![
                Cell::new(spec_name.to_string()).set_alignment(CellAlignment::Left),
                Cell::new("").set_alignment(CellAlignment::Left),
                Cell::new("").set_alignment(CellAlignment::Left),
            ]);

            let (name_content, type_content, value_content) = self.build_data_content(group);

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

    fn build_data_content(&self, group: &lemma::DataGroup) -> (String, String, String) {
        let mut name_lines = Vec::new();
        let mut type_lines = Vec::new();
        let mut value_lines = Vec::new();

        for data in &group.data {
            let value_str = match &data.value {
                DataValue::Definition { bound_value, .. } => bound_value
                    .as_ref()
                    .map(|lit| self.format_literal(lit))
                    .unwrap_or_default(),
            };
            name_lines.push(data.path.to_string());
            type_lines.push(Self::data_type_str(&data.value));
            value_lines.push(value_str);
        }

        (
            name_lines.join("\n"),
            type_lines.join("\n"),
            value_lines.join("\n"),
        )
    }

    fn data_type_str(value: &DataValue) -> String {
        match value {
            DataValue::Definition { schema_type, .. } => schema_type.name(),
        }
    }

    fn format_literal(&self, lit: &LiteralValue) -> String {
        match &lit.value {
            ValueKind::Text(s) => s.clone(),
            _ => lit.to_string(),
        }
    }

    fn format_rule_result(&self, result: &RuleResult) -> String {
        let mut rows: Vec<String> = Vec::new();
        let mut expanded: HashSet<String> = HashSet::new();

        if let Some(explanation) = &result.explanation {
            self.render_node(explanation.tree.as_ref(), "", &mut rows, &mut expanded);
        }

        let mut table = Table::new();
        table.load_preset(presets::UTF8_FULL);
        table.set_style(super_table::TableComponent::MiddleIntersections, '┼');
        table.set_style(super_table::TableComponent::HorizontalLines, '─');

        let header = format!(
            "{}: {}",
            result.rule.name,
            self.highlight_value(&self.format_result_inline(&result.result))
        );
        table.add_row(vec![Cell::new(&header).set_alignment(CellAlignment::Left)]);

        if !rows.is_empty() {
            let content = rows.join("\n");
            table.add_row(vec![Cell::new(content).set_alignment(CellAlignment::Left)]);
        }

        let source = &result.rule.source_location;
        let location = format!("Source: {}:{}", source.source_type, source.span.line);
        table.add_row(vec![
            Cell::new(self.gray(&location)).set_alignment(CellAlignment::Left)
        ]);

        table.to_string()
    }

    fn render_node(
        &self,
        node: &ExplanationNode,
        indent: &str,
        rows: &mut Vec<String>,
        expanded: &mut HashSet<String>,
    ) {
        let mut ctx = RenderContext {
            rows,
            expanded,
            indent,
        };
        match node {
            ExplanationNode::Value { value, source, .. } => {
                self.render_value(value, source, &mut ctx);
            }
            ExplanationNode::RuleReference {
                rule_path,
                result,
                expansion,
                ..
            } => {
                self.render_rule_reference(rule_path, result, expansion, Connector::Last, &mut ctx);
            }
            ExplanationNode::Computation {
                kind,
                conversion_steps,
                expression,
                original_expression,
                operands,
                ..
            } => match kind {
                ComputationKind::UnitConversion { .. } => {
                    self.render_unit_conversion_computation(conversion_steps, operands, &mut ctx);
                }
                _ => {
                    self.render_computation(expression, original_expression, operands, &mut ctx);
                }
            },
            ExplanationNode::Branches {
                matched,
                non_matched,
                ..
            } => {
                self.render_branches(matched, non_matched, &mut ctx);
            }
            ExplanationNode::Condition {
                expression,
                original_expression,
                result,
                operands,
                ..
            } => {
                self.render_condition(expression, original_expression, *result, operands, &mut ctx);
            }
            ExplanationNode::Veto { message, .. } => {
                self.render_veto(message, &mut ctx);
            }
        }
    }

    fn render_node_with_connector(
        &self,
        node: &ExplanationNode,
        indent: &str,
        connector: Connector,
        rows: &mut Vec<String>,
        expanded: &mut HashSet<String>,
    ) {
        let mut ctx = RenderContext {
            rows,
            expanded,
            indent,
        };
        match node {
            ExplanationNode::Value { value, source, .. } => {
                let display = match source {
                    ValueSource::Data { data_ref } => {
                        format!("{} is {}", data_ref, self.format_literal_inline(value))
                    }
                    ValueSource::Literal | ValueSource::Computed => {
                        self.format_literal_inline(value)
                    }
                };
                ctx.rows.push(format!(
                    "{}{} {}",
                    ctx.indent,
                    self.connector_str(connector),
                    display
                ));
            }
            ExplanationNode::RuleReference {
                rule_path,
                result,
                expansion,
                ..
            } => {
                self.render_rule_reference(
                    rule_path,
                    result,
                    expansion.as_ref(),
                    connector,
                    &mut ctx,
                );
            }
            _ => {
                self.render_node(node, indent, rows, expanded);
            }
        }
    }

    fn render_value(&self, value: &LiteralValue, source: &ValueSource, ctx: &mut RenderContext) {
        let display = match source {
            ValueSource::Data { data_ref } => {
                format!("{} is {}", data_ref, self.format_literal_inline(value))
            }
            ValueSource::Literal | ValueSource::Computed => self.format_literal_inline(value),
        };
        ctx.rows.push(format!("{}└─ {}", ctx.indent, display));
    }

    fn render_rule_reference(
        &self,
        rule_path: &lemma::planning::semantics::RulePath,
        result: &OperationResult,
        expansion: &ExplanationNode,
        connector: Connector,
        ctx: &mut RenderContext,
    ) {
        let rule_key = rule_path.to_string();
        let result_str = self.highlight_value(&self.format_result_inline(result));
        ctx.rows.push(format!(
            "{}{} {}: {}",
            ctx.indent,
            self.connector_str(connector),
            rule_path,
            result_str
        ));

        if ctx.expanded.insert(rule_key) {
            let child_indent = self.child_indent(ctx.indent, connector);
            self.render_node(expansion, &child_indent, ctx.rows, ctx.expanded);
        }
    }

    fn render_unit_conversion_computation(
        &self,
        conversion_steps: &[ConversionExplanationStep],
        operands: &[ExplanationNode],
        ctx: &mut RenderContext,
    ) {
        assert!(
            !conversion_steps.is_empty(),
            "BUG: UnitConversion computation must have conversion_steps"
        );
        let steps_count = conversion_steps.len();
        for (index, step) in conversion_steps.iter().enumerate() {
            if index == 0 {
                ctx.rows.push(format!("{}{}", ctx.indent, step.text));
            } else {
                let step_indent = format!("{}{}", "   ".repeat(index), ctx.indent);
                let connector = if index + 1 == steps_count && operands.is_empty() {
                    Connector::Last
                } else {
                    Connector::Branch
                };
                ctx.rows.push(format!(
                    "{}{} {}",
                    step_indent,
                    self.connector_str(connector),
                    step.text
                ));
            }
        }

        if operands.is_empty() {
            return;
        }

        let operand_indent = format!("{}   ", "   ".repeat(steps_count) + ctx.indent);
        let len = operands.len();
        for (index, child) in operands.iter().enumerate() {
            let connector = if index == len - 1 {
                Connector::Last
            } else {
                Connector::Branch
            };
            self.render_node_with_connector(
                child,
                &operand_indent,
                connector,
                ctx.rows,
                ctx.expanded,
            );
        }
    }

    fn render_computation(
        &self,
        expression: &str,
        original_expression: &str,
        operands: &[ExplanationNode],
        ctx: &mut RenderContext,
    ) {
        push_expression_header_lines(ctx.rows, ctx.indent, expression, original_expression);

        let child_indent = format!("{}   ", ctx.indent);
        let expandable = Self::collect_expandable_operands(operands);

        let len = expandable.len();
        for (i, child) in expandable.iter().enumerate() {
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

    /// Recursively flatten nested Computation operands so that
    /// `(a + b) + c` expands as `[a, b, c]` instead of nesting.
    fn collect_expandable_operands(operands: &[ExplanationNode]) -> Vec<&ExplanationNode> {
        let mut result = Vec::new();
        for op in operands {
            match op {
                ExplanationNode::Value { source, .. } => {
                    if matches!(source, ValueSource::Data { .. }) {
                        result.push(op);
                    }
                }
                ExplanationNode::Computation {
                    operands: nested, ..
                } => {
                    result.extend(Self::collect_expandable_operands(nested));
                }
                other => result.push(other),
            }
        }
        result
    }

    fn render_branches(
        &self,
        matched: &lemma::evaluation::explanation::Branch,
        non_matched: &[NonMatchedBranch],
        ctx: &mut RenderContext,
    ) {
        enum BranchItem<'a> {
            Matched(&'a lemma::evaluation::explanation::Branch),
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

        // Collect non-matched branches so we can deduplicate operand expansion across them.
        let non_matched_branches: Vec<&NonMatchedBranch> = all_branches
            .iter()
            .filter_map(|(_, item)| {
                if let BranchItem::NonMatched(b) = item {
                    Some(*b)
                } else {
                    None
                }
            })
            .collect();

        for (_, branch_item) in &all_branches {
            match branch_item {
                BranchItem::Matched(branch) => {
                    let has_condition = branch.condition.is_some();

                    if let Some(condition) = &branch.condition {
                        ctx.rows.push(format!(
                            "{}→ {}",
                            ctx.indent,
                            self.extract_condition_text(condition)
                        ));
                    }

                    if !matches!(branch.result.as_ref(), ExplanationNode::Value { .. }) {
                        let result_indent = if has_condition {
                            format!("{}   ", ctx.indent)
                        } else {
                            ctx.indent.to_string()
                        };
                        self.render_node(&branch.result, &result_indent, ctx.rows, ctx.expanded);
                    }
                }
                BranchItem::NonMatched(branch) => {
                    ctx.rows.push(format!(
                        "{}→ {}",
                        ctx.indent,
                        self.extract_condition_text(&branch.condition)
                    ));
                }
            }
        }

        // Render operands from all non-matched conditions once, deduplicated by rule path.
        if !non_matched_branches.is_empty() {
            let condition_indent = format!("{}  ", ctx.indent);
            let operands = Self::collect_operands_dedup(
                non_matched_branches.iter().map(|b| b.condition.as_ref()),
            );
            let len = operands.len();
            for (i, node) in operands.iter().enumerate() {
                let connector = if i == len - 1 {
                    Connector::Last
                } else {
                    Connector::Branch
                };
                self.render_node_with_connector(
                    node,
                    &condition_indent,
                    connector,
                    ctx.rows,
                    ctx.expanded,
                );
            }
        }
    }

    /// Collect RuleReference operands from condition nodes, deduplicated by rule path (first occurrence order).
    fn collect_operands_dedup<'a>(
        condition_nodes: impl Iterator<Item = &'a ExplanationNode>,
    ) -> Vec<&'a ExplanationNode> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for node in condition_nodes {
            let operands: &[ExplanationNode] = match node {
                ExplanationNode::Computation { operands, .. }
                | ExplanationNode::Condition { operands, .. } => operands.as_ref(),
                _ => continue,
            };
            for op in operands {
                if let ExplanationNode::RuleReference { rule_path, .. } = op {
                    if seen.insert(rule_path.to_string()) {
                        out.push(op);
                    }
                }
            }
        }
        out
    }

    fn render_condition(
        &self,
        expression: &str,
        original_expression: &str,
        _result: bool,
        operands: &[ExplanationNode],
        ctx: &mut RenderContext,
    ) {
        push_expression_header_lines(ctx.rows, ctx.indent, expression, original_expression);

        let child_indent = format!("{}   ", ctx.indent);
        let expandable = Self::collect_expandable_operands(operands);

        let len = expandable.len();
        for (i, child) in expandable.iter().enumerate() {
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
        let msg = match message {
            Some(m) => format!("veto: {}", m),
            None => "veto".to_string(),
        };
        ctx.rows.push(format!("{}└─ {}", ctx.indent, msg));
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

    fn format_result_inline(&self, result: &OperationResult) -> String {
        match result {
            OperationResult::Value(v) => self.format_literal_inline(v),
            OperationResult::Veto(reason) => format!("Veto: {reason}"),
        }
    }

    fn format_literal_inline(&self, lit: &LiteralValue) -> String {
        match &lit.value {
            ValueKind::Number(n) => {
                let decimals_opt = lit.lemma_type.decimal_places();
                format_rational(n, decimals_opt)
            }
            ValueKind::Quantity(n, unit, _decomposition) => {
                let decimals_opt = lit.lemma_type.decimal_places();
                format!("{} {}", format_rational(n, decimals_opt), unit)
            }
            ValueKind::Ratio(r, unit_opt) => {
                if unit_opt.is_some() {
                    lit.display_value()
                } else {
                    format_rational(r, lit.lemma_type.decimal_places())
                }
            }
            ValueKind::Text(s) => format!("\"{}\"", s),
            ValueKind::Boolean(b) => b.to_string(),
            ValueKind::Date(d) => d.to_string(),
            ValueKind::Time(t) => t.to_string(),
            ValueKind::Calendar(value, unit) => {
                format!("{} {}", format_rational(value, None), unit)
            }
            ValueKind::Range(left, right) => {
                format!(
                    "{}...{}",
                    self.format_literal_inline(left.as_ref()),
                    self.format_literal_inline(right.as_ref())
                )
            }
        }
    }

    fn extract_condition_text(&self, node: &ExplanationNode) -> String {
        match node {
            ExplanationNode::Computation {
                original_expression,
                ..
            } => original_expression.clone(),
            ExplanationNode::Condition {
                original_expression,
                ..
            } => original_expression.clone(),
            ExplanationNode::Value { value, source, .. } => match source {
                ValueSource::Data { data_ref } => data_ref.to_string(),
                ValueSource::Literal | ValueSource::Computed => value.to_string(),
            },
            ExplanationNode::RuleReference { rule_path, .. } => rule_path.to_string(),
            ExplanationNode::Branches { .. } => "<branches>".to_string(),
            ExplanationNode::Veto { message, .. } => {
                message.clone().unwrap_or_else(|| "veto".to_string())
            }
        }
    }

    fn gray(&self, text: &str) -> String {
        format!("\x1b[90m{}\x1b[0m", text)
    }

    fn highlight_value(&self, text: &str) -> String {
        format!("\x1b[38;2;80;180;220m{}\x1b[0m", text)
    }
}

/// One line when simplified expression equals source; two lines when they differ.
fn push_expression_header_lines(
    rows: &mut Vec<String>,
    indent: &str,
    expression: &str,
    original_expression: &str,
) {
    if expression == original_expression {
        rows.push(format!("{}└─ {}", indent, expression));
    } else {
        rows.push(format!("{}├─ {}", indent, expression));
        rows.push(format!("{}└─ {}", indent, original_expression));
    }
}

fn format_rational(rational: &RationalInteger, decimals: Option<u8>) -> String {
    match commit_rational_to_decimal(rational) {
        Ok(decimal) => format_decimal(&decimal, decimals),
        Err(_) => rational_to_display_str(rational),
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

#[cfg(test)]
mod tests {
    use super::push_expression_header_lines;

    #[test]
    fn expression_header_single_line_when_expression_equals_original() {
        let mut rows = Vec::new();
        push_expression_header_lines(&mut rows, "", "3000 * 12", "3000 * 12");
        assert_eq!(rows, vec!["└─ 3000 * 12"]);
    }

    #[test]
    fn expression_header_two_lines_when_simplified_differs_from_source() {
        let mut rows = Vec::new();
        push_expression_header_lines(&mut rows, "", "36000", "3000 * 12");
        assert_eq!(rows, vec!["├─ 36000", "└─ 3000 * 12"]);
    }
}
