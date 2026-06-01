use lemma::{
    type_detail_lines, BindingDataValue, ComputationKind, ConversionTraceStep, DataEntry,
    LiteralValue, OperationResult, Response, RulePath, RuleResult, SpecSchema, TraceBranch,
    TraceNode, TraceNonMatchedBranch, TraceValueSource, ValueKind,
};
use std::collections::HashSet;
use super_table::{presets, Cell, CellAlignment, Table};

pub struct RepositorySpecGroup<'a> {
    pub repository: Option<&'a str>,
    pub specs: &'a [String],
}

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
            return format!("{}\n", self.format_rule_display(result));
        }

        let mut table = Table::new();
        table.load_preset(presets::UTF8_FULL);
        table.set_style(super_table::TableComponent::MiddleIntersections, '┼');
        table.set_style(super_table::TableComponent::HorizontalLines, '─');
        for result in response.results.values() {
            table.add_row(vec![
                Cell::new(&result.rule.name).set_alignment(CellAlignment::Left),
                Cell::new(self.format_rule_display(result)).set_alignment(CellAlignment::Left),
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

    pub fn format_spec_schema(&self, schema: &SpecSchema) -> String {
        let mut output = String::new();

        // Spec header: name on first line, then optional indented lines
        output.push_str(&schema.spec);
        output.push('\n');
        if let Some(commentary) = &schema.commentary {
            output.push_str(&format!("  {}\n", commentary));
        }
        if let Some(effective) = &schema.effective {
            output.push_str(&format!("  effective: {}\n", effective));
        }
        if schema.versions.len() > 1 {
            let version_strs: Vec<String> = schema.versions.iter().map(|v| v.to_string()).collect();
            output.push_str(&format!("  versions: {}\n", version_strs.join(", ")));
        }

        if schema.data.is_empty() && schema.rules.is_empty() {
            output.push_str("\n  (no data or rules)\n");
            return output;
        }

        // Data section
        if !schema.data.is_empty() {
            output.push('\n');
            output.push_str("Data\n");
            let max_name_width = schema.data.keys().map(|name| name.len()).max().unwrap_or(0);
            for (name, entry) in &schema.data {
                let first_line = Self::build_entry_first_line(name, entry);
                output.push_str(&format!(
                    "  {:<width$}  {}\n",
                    name,
                    first_line,
                    width = max_name_width
                ));
                let property_indent = " ".repeat(2 + max_name_width + 2 + 2);
                for line in type_detail_lines(&entry.lemma_type.specifications) {
                    output.push_str(&format!("{}{}\n", property_indent, line));
                }
                let help = entry.lemma_type.specifications.help();
                if !help.is_empty() {
                    output.push_str(&format!("{}help: {}\n", property_indent, help));
                }
            }
        }

        // Rules section
        if !schema.rules.is_empty() {
            output.push('\n');
            output.push_str("Rules\n");
            let max_name_width = schema
                .rules
                .keys()
                .map(|name| name.len())
                .max()
                .unwrap_or(0);
            for (name, rule_type) in &schema.rules {
                let mut detail = rule_type.specifications.to_string();
                if let Some(ref type_name) = rule_type.name {
                    if type_name != name {
                        detail.push_str(&format!(" ({})", type_name));
                    }
                }
                output.push_str(&format!(
                    "  {:<width$}  {}\n",
                    name,
                    detail,
                    width = max_name_width
                ));
            }
        }

        output
    }

    fn build_entry_first_line(data_name: &str, entry: &DataEntry) -> String {
        let mut line = entry.lemma_type.specifications.to_string();
        if let Some(ref type_name) = entry.lemma_type.name {
            if type_name != data_name {
                line.push_str(&format!(" ({})", type_name));
            }
        }
        if let Some(bound) = &entry.bound_value {
            line.push_str(&format!(" = {}", bound));
        } else if let Some(default) = &entry.default {
            line.push_str(&format!(" = {}", default));
        }
        line
    }

    pub fn format_repository_spec_list(&self, groups: &[RepositorySpecGroup<'_>]) -> String {
        let mut output = String::new();
        for (index, group) in groups.iter().enumerate() {
            if index > 0 {
                output.push('\n');
            }
            match group.repository {
                None => {
                    for spec in group.specs {
                        output.push_str(spec);
                        output.push('\n');
                    }
                }
                Some(repository) => {
                    output.push_str(repository);
                    output.push('\n');
                    for spec in group.specs {
                        output.push_str("  ");
                        output.push_str(spec);
                        output.push('\n');
                    }
                }
            }
        }
        output
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
                BindingDataValue::Definition { bound_value, .. } => bound_value
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

    fn data_type_str(value: &BindingDataValue) -> String {
        match value {
            BindingDataValue::Definition { schema_type, .. } => schema_type.name(),
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

        if let Some(trace) = &result.trace {
            self.render_node(trace.tree.as_ref(), "", &mut rows, &mut expanded);
        }

        let mut table = Table::new();
        table.load_preset(presets::UTF8_FULL);
        table.set_style(super_table::TableComponent::MiddleIntersections, '┼');
        table.set_style(super_table::TableComponent::HorizontalLines, '─');

        let header = format!(
            "{}: {}",
            result.rule.name,
            self.highlight_value(&self.format_rule_display(result))
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
        node: &TraceNode,
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
            TraceNode::Value { value, source, .. } => {
                self.render_value(value, source, &mut ctx);
            }
            TraceNode::RuleReference {
                rule_path,
                result,
                expansion,
                ..
            } => {
                self.render_rule_reference(rule_path, result, expansion, Connector::Last, &mut ctx);
            }
            TraceNode::Computation {
                kind,
                conversion_steps,
                expression,
                operands,
                ..
            } => match kind {
                ComputationKind::UnitConversion { .. } => {
                    self.render_unit_conversion_computation(conversion_steps, operands, &mut ctx);
                }
                _ => {
                    self.render_computation(expression, operands, &mut ctx);
                }
            },
            TraceNode::Branches {
                matched,
                non_matched,
                ..
            } => {
                self.render_branches(matched, non_matched, &mut ctx);
            }
            TraceNode::Veto { message, .. } => {
                self.render_veto(message, &mut ctx);
            }
        }
    }

    fn render_node_with_connector(
        &self,
        node: &TraceNode,
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
            TraceNode::Value { value, source, .. } => {
                let display = match source {
                    TraceValueSource::Data { data_ref } => {
                        format!("{} is {}", data_ref, self.format_literal_inline(value))
                    }
                    TraceValueSource::Literal | TraceValueSource::Computed => {
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
            TraceNode::RuleReference {
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

    fn render_value(
        &self,
        value: &LiteralValue,
        source: &TraceValueSource,
        ctx: &mut RenderContext,
    ) {
        let display = match source {
            TraceValueSource::Data { data_ref } => {
                format!("{} is {}", data_ref, self.format_literal_inline(value))
            }
            TraceValueSource::Literal | TraceValueSource::Computed => {
                self.format_literal_inline(value)
            }
        };
        ctx.rows.push(format!("{}└─ {}", ctx.indent, display));
    }

    fn render_rule_reference(
        &self,
        rule_path: &RulePath,
        result: &OperationResult,
        expansion: &TraceNode,
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
        conversion_steps: &[ConversionTraceStep],
        operands: &[TraceNode],
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
        operands: &[TraceNode],
        ctx: &mut RenderContext,
    ) {
        ctx.rows.push(format!("{}└─ {}", ctx.indent, expression));

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
    fn collect_expandable_operands(operands: &[TraceNode]) -> Vec<&TraceNode> {
        let mut result = Vec::new();
        for op in operands {
            match op {
                TraceNode::Value { source, .. } => {
                    if matches!(source, TraceValueSource::Data { .. }) {
                        result.push(op);
                    }
                }
                TraceNode::Computation {
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
        matched: &TraceBranch,
        non_matched: &[TraceNonMatchedBranch],
        ctx: &mut RenderContext,
    ) {
        enum TraceBranchItem<'a> {
            Matched(&'a TraceBranch),
            NonMatched(&'a TraceNonMatchedBranch),
        }

        let mut all_branches: Vec<((bool, usize), TraceBranchItem)> = Vec::new();

        let matched_key = match matched.clause_index {
            None => (false, 0),
            Some(idx) => (true, idx),
        };
        all_branches.push((matched_key, TraceBranchItem::Matched(matched)));

        for branch in non_matched {
            let key = match branch.clause_index {
                None => (false, 0),
                Some(idx) => (true, idx),
            };
            all_branches.push((key, TraceBranchItem::NonMatched(branch)));
        }

        all_branches.sort_by_key(|((is_some, idx), _)| (*is_some, *idx));

        // Collect non-matched branches so we can deduplicate operand expansion across them.
        let non_matched_branches: Vec<&TraceNonMatchedBranch> = all_branches
            .iter()
            .filter_map(|(_, item)| {
                if let TraceBranchItem::NonMatched(b) = item {
                    Some(*b)
                } else {
                    None
                }
            })
            .collect();

        for (_, branch_item) in &all_branches {
            match branch_item {
                TraceBranchItem::Matched(branch) => {
                    let has_condition = branch.condition.is_some();

                    if let Some(condition) = &branch.condition {
                        ctx.rows.push(format!(
                            "{}→ {}",
                            ctx.indent,
                            self.extract_condition_text(condition)
                        ));
                    }

                    if !matches!(branch.result.as_ref(), TraceNode::Value { .. }) {
                        let result_indent = if has_condition {
                            format!("{}   ", ctx.indent)
                        } else {
                            ctx.indent.to_string()
                        };
                        self.render_node(&branch.result, &result_indent, ctx.rows, ctx.expanded);
                    }
                }
                TraceBranchItem::NonMatched(branch) => {
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
        condition_nodes: impl Iterator<Item = &'a TraceNode>,
    ) -> Vec<&'a TraceNode> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for node in condition_nodes {
            let operands: &[TraceNode] = match node {
                TraceNode::Computation { operands, .. } => operands.as_ref(),
                _ => continue,
            };
            for op in operands {
                if let TraceNode::RuleReference { rule_path, .. } = op {
                    if seen.insert(rule_path.to_string()) {
                        out.push(op);
                    }
                }
            }
        }
        out
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

    fn format_rule_display(&self, result: &RuleResult) -> String {
        if result.vetoed {
            return result
                .veto_reason
                .clone()
                .expect("BUG: vetoed rule result must have veto_reason");
        }
        result
            .display
            .clone()
            .expect("BUG: non-veto rule result must have display after materialization")
    }

    fn format_result_inline(&self, result: &OperationResult) -> String {
        match result {
            OperationResult::Value(v) => self.format_literal_inline(v),
            OperationResult::Veto(reason) => format!("Veto: {reason}"),
        }
    }

    fn format_literal_inline(&self, lit: &LiteralValue) -> String {
        lit.display_value()
    }

    fn extract_condition_text(&self, node: &TraceNode) -> String {
        match node {
            TraceNode::Computation { expression, .. } => expression.clone(),
            TraceNode::Value { value, source, .. } => match source {
                TraceValueSource::Data { data_ref } => data_ref.to_string(),
                TraceValueSource::Literal | TraceValueSource::Computed => value.to_string(),
            },
            TraceNode::RuleReference { rule_path, .. } => rule_path.to_string(),
            TraceNode::Branches { .. } => "<branches>".to_string(),
            TraceNode::Veto { message, .. } => {
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
