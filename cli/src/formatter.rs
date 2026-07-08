use lemma::{
    format_explanation, type_detail_lines, BindingDataValue, DataEntry, LiteralValue, Response,
    RuleResult, SpecSchema, ValueKind,
};
use super_table::{presets, Cell, CellAlignment, Table};

pub struct RepositorySpecGroup<'a> {
    pub repository: Option<&'a str>,
    pub specs: &'a [String],
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

    pub fn response_json_value(
        &self,
        response: &Response,
        include_explanations: bool,
    ) -> serde_json::Value {
        let mut value =
            serde_json::to_value(response).expect("BUG: failed to serialize response JSON");
        if !include_explanations {
            if let Some(results) = value.get_mut("results").and_then(|r| r.as_object_mut()) {
                for rule in results.values_mut() {
                    if let Some(obj) = rule.as_object_mut() {
                        obj.remove("explanation");
                    }
                }
            }
            if let Some(obj) = value.as_object_mut() {
                obj.remove("data");
            }
        }
        value
    }

    pub fn serialize_response_json(
        &self,
        response: &Response,
        include_explanations: bool,
    ) -> String {
        serde_json::to_string_pretty(&self.response_json_value(response, include_explanations))
            .expect("BUG: failed to serialize response JSON")
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
        if let Some(v) = &entry.prefilled {
            line.push_str(&format!(" = {}", v));
        } else if let Some(v) = &entry.supplied {
            line.push_str(&format!(" = {} (supplied)", v));
        } else if let Some(default) = &entry.default {
            line.push_str(&format!(" = {} (default)", default));
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
                BindingDataValue::Definition { value, .. } => value
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
        let mut table = Table::new();
        table.load_preset(presets::UTF8_FULL);
        table.set_style(super_table::TableComponent::MiddleIntersections, '┼');
        table.set_style(super_table::TableComponent::HorizontalLines, '─');

        if let Some(explanation) = &result.explanation {
            table.add_row(vec![
                Cell::new(format_explanation(explanation)).set_alignment(CellAlignment::Left)
            ]);
        } else {
            let header = format!(
                "{}: {}",
                result.rule.name,
                self.highlight_value(&self.format_rule_display(result))
            );
            table.add_row(vec![Cell::new(&header).set_alignment(CellAlignment::Left)]);
        }

        let source = &result.rule.source_location;
        let location = format!("Source: {}:{}", source.source_type, source.span.line);
        table.add_row(vec![
            Cell::new(self.gray(&location)).set_alignment(CellAlignment::Left)
        ]);

        table.to_string()
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

    fn gray(&self, text: &str) -> String {
        format!("\x1b[90m{}\x1b[0m", text)
    }

    fn highlight_value(&self, text: &str) -> String {
        format!("\x1b[38;2;80;180;220m{}\x1b[0m", text)
    }
}
