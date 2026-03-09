use lemma::evaluation::proof::{NonMatchedBranch, ProofNode, ValueSource};
use lemma::planning::semantics::{FactPath, FactValue, TypeSpecification, ValueKind};
use lemma::{ExecutionPlan, LiteralValue, OperationResult, Response, RuleResult, SpecSchema};
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
        if !response.facts.is_empty() {
            output.push_str("Facts\n");
            output.push_str(&self.format_facts_tree(&response.facts, &response.spec_name));
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

    pub fn format_spec_inspection(&self, plan: &ExecutionPlan, hash: Option<&str>) -> String {
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
            Cell::new(&plan.spec_name).set_alignment(CellAlignment::Left)
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

        if let Some(h) = hash {
            content_lines.push(format!("hash: {}", h));
        }

        table.add_row(vec![
            Cell::new(content_lines.join("\n")).set_alignment(CellAlignment::Left)
        ]);

        format!("{}\n", table)
    }

    pub fn format_workspace_summary(&self, file_count: usize, schemas: &[SpecSchema]) -> String {
        let mut output = String::new();
        let spec_count = schemas.len();
        let file_word = if file_count == 1 { "file" } else { "files" };
        let spec_word = if spec_count == 1 { "spec" } else { "specs" };
        output.push_str(&format!(
            "Found {} {} in {} {}\n",
            spec_count, spec_word, file_count, file_word
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
                Cell::new(&schema.spec).set_alignment(CellAlignment::Left),
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

    fn format_facts_tree(&self, facts_groups: &[lemma::Facts], spec_name: &str) -> String {
        let mut output = String::new();

        for group in facts_groups {
            if group.facts.is_empty() {
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

            let (name_content, type_content, value_content) = self.build_facts_content(group);

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

    fn build_facts_content(&self, group: &lemma::Facts) -> (String, String, String) {
        let mut name_lines = Vec::new();
        let mut type_lines = Vec::new();
        let mut value_lines = Vec::new();

        for fact in &group.facts {
            let value_str = match &fact.value {
                FactValue::Literal(lit) => self.format_literal(lit),
                FactValue::SpecReference(spec_ref) => format!("spec {}", spec_ref),
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

    fn fact_type_str(value: &FactValue) -> String {
        match value {
            FactValue::Literal(lit) => lit.lemma_type.name(),
            FactValue::TypeDeclaration { resolved_type } => resolved_type.name(),
            FactValue::SpecReference(spec_ref) => format!("spec {}", spec_ref),
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

        if let Some(proof) = &result.proof {
            self.render_node(&proof.tree, "", &mut rows, &mut expanded);
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
        let location = format!(
            "Source: {}:{}:{}",
            source.attribute, source.span.line, source.span.col
        );
        table.add_row(vec![
            Cell::new(self.gray(&location)).set_alignment(CellAlignment::Left)
        ]);

        table.to_string()
    }

    fn render_node(
        &self,
        node: &ProofNode,
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
        rows: &mut Vec<String>,
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
                    ValueSource::Fact { fact_ref } => {
                        format!("{} is {}", fact_ref, self.format_literal_inline(value))
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
            ValueSource::Fact { fact_ref } => {
                format!("{} is {}", fact_ref, self.format_literal_inline(value))
            }
            ValueSource::Literal | ValueSource::Computed => self.format_literal_inline(value),
        };
        ctx.rows.push(format!("{}└─ {}", ctx.indent, display));
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

    fn render_computation(
        &self,
        expression: &str,
        original_expression: &str,
        operands: &[ProofNode],
        ctx: &mut RenderContext,
    ) {
        ctx.rows.push(format!("{}├─ {}", ctx.indent, expression));
        ctx.rows
            .push(format!("{}└─ {}", ctx.indent, original_expression));

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
    fn collect_expandable_operands(operands: &[ProofNode]) -> Vec<&ProofNode> {
        let mut result = Vec::new();
        for op in operands {
            match op {
                ProofNode::Value { .. } => {}
                ProofNode::Computation {
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
        condition_nodes: impl Iterator<Item = &'a ProofNode>,
    ) -> Vec<&'a ProofNode> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for node in condition_nodes {
            let operands: &[ProofNode] = match node {
                ProofNode::Computation { operands, .. } | ProofNode::Condition { operands, .. } => {
                    operands.as_ref()
                }
                _ => continue,
            };
            for op in operands {
                if let ProofNode::RuleReference { rule_path, .. } = op {
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
        operands: &[ProofNode],
        ctx: &mut RenderContext,
    ) {
        ctx.rows.push(format!("{}├─ {}", ctx.indent, expression));
        ctx.rows
            .push(format!("{}└─ {}", ctx.indent, original_expression));

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
            OperationResult::Veto(msg) => match msg {
                Some(m) => format!("Veto: {}", m),
                None => "Veto".to_string(),
            },
        }
    }

    fn format_literal_inline(&self, lit: &LiteralValue) -> String {
        match &lit.value {
            ValueKind::Number(n) => {
                let decimals_opt = lit.lemma_type.decimal_places();
                format_decimal(n, decimals_opt)
            }
            ValueKind::Scale(n, unit) => {
                let decimals_opt = lit.lemma_type.decimal_places();
                format!("{} {}", format_decimal(n, decimals_opt), unit)
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
                            "%"
                        } else {
                            unit_name
                        };
                        format!(
                            "{}{}",
                            format_decimal(&display_value, decimals_opt),
                            display_unit
                        )
                    }
                    None => format_decimal(r, decimals_opt),
                }
            }
            ValueKind::Text(s) => format!("\"{}\"", s),
            ValueKind::Boolean(b) => b.to_string(),
            ValueKind::Date(d) => d.to_string(),
            ValueKind::Time(t) => t.to_string(),
            ValueKind::Duration(value, unit) => {
                format!("{} {}", format_decimal(value, None), unit)
            }
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
        format!("\x1b[90m{}\x1b[0m", text)
    }

    fn highlight_value(&self, text: &str) -> String {
        format!("\x1b[38;2;80;180;220m{}\x1b[0m", text)
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
