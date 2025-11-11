use comfy_table::{presets::UTF8_FULL, Attribute, Cell, CellAlignment, ContentArrangement, Table};
use crossterm::style::Stylize;
use lemma::{LemmaDoc, LemmaFact, LemmaRule, OperationRecord, Response};
use std::fmt::Write as _;

pub struct Formatter {
    use_colors: bool,
}

impl Default for Formatter {
    fn default() -> Self {
        Self { use_colors: true }
    }
}

impl Formatter {
    pub fn format_response(&self, response: &Response, raw: bool) -> String {
        if raw {
            self.format_raw(response)
        } else {
            self.format_table(response)
        }
    }

    fn format_raw(&self, response: &Response) -> String {
        let mut output = String::default();

        for result in &response.results {
            if let Some(ref value) = result.result {
                output.push_str(&value.to_string());
                output.push('\n');
            }
        }

        output
    }

    fn format_table(&self, response: &Response) -> String {
        let mut table = Table::new();
        table
            .load_preset(UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic);

        table.set_header(vec![
            Cell::new("Rule").add_attribute(Attribute::Bold),
            Cell::new("Evaluation")
                .add_attribute(Attribute::Bold)
                .set_alignment(CellAlignment::Left),
        ]);

        for result in &response.results {
            let rule_cell = Cell::new(&result.rule_name);

            let verdict_cell = if let Some(ref value) = result.result {
                let mut content = format!("{value}\n");

                if !result.operations.is_empty() {
                    content.push('\n');
                    for (i, step) in result.operations.iter().enumerate() {
                        content.push_str(&self.format_operation_step(i, step));
                    }
                }

                Cell::new(content.trim_end()).set_alignment(CellAlignment::Left)
            } else if let Some(ref missing) = result.missing_facts {
                let facts_str = missing.join("\n  - ");
                Cell::new(format!("Missing facts:\n  - {facts_str}"))
                    .set_alignment(CellAlignment::Left)
            } else if let Some(ref veto_msg) = result.veto_message {
                Cell::new(format!("✗ {veto_msg}")).set_alignment(CellAlignment::Left)
            } else {
                Cell::new("[no result]").set_alignment(CellAlignment::Left)
            };

            table.add_row(vec![rule_cell, verdict_cell]);
        }

        format!("{table}\n")
    }

    fn format_operation_step(&self, index: usize, step: &OperationRecord) -> String {
        match step {
            OperationRecord::FactUsed { name, value } => {
                format!("  {index:>2}. fact {name} = {value}\n")
            }
            OperationRecord::RuleUsed { name, value } => {
                format!("  {index:>2}. rule {name} = {value}\n")
            }
            OperationRecord::OperationExecuted {
                operation,
                inputs,
                result,
                unless_clause_index,
            } => {
                let inputs_str = inputs
                    .iter()
                    .map(std::string::ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");

                if let Some(clause_idx) = unless_clause_index {
                    format!(
                        "  {index:>2}. unless #{clause_idx}: {operation}({inputs_str}) → {result}\n"
                    )
                } else {
                    format!("  {index:>2}. {operation}({inputs_str}) → {result}\n")
                }
            }
            OperationRecord::UnlessClauseEvaluated {
                index: clause_index,
                matched,
                result_if_matched,
            } => {
                if *matched {
                    if let Some(value) = result_if_matched {
                        format!("  {index:>2}. unless clause {clause_index} matched → {value}\n")
                    } else {
                        format!("  {index:>2}. unless clause {clause_index} matched (veto)\n")
                    }
                } else {
                    format!("  {index:>2}. unless clause {clause_index} skipped\n")
                }
            }
            OperationRecord::DefaultValue { value } => {
                format!("  {index:>2}. default = {value}\n")
            }
            OperationRecord::FinalResult { value } => {
                format!("  {index:>2}. result = {value}\n")
            }
        }
    }

    pub fn format_document_inspection(
        &self,
        doc: &LemmaDoc,
        facts: &[&LemmaFact],
        rules: &[&LemmaRule],
    ) -> String {
        let mut output = String::default();

        output.push_str(&self.section_divider());
        output.push_str(&self.style_header(&format!("  {}", doc.name)));
        output.push('\n');
        output.push_str(&self.section_divider());
        output.push('\n');

        if let Some(commentary) = &doc.commentary {
            let lines: Vec<&str> = commentary.lines().collect();
            for line in lines {
                if self.use_colors {
                    let _ = writeln!(output, "  {}", line.dark_grey());
                } else {
                    let _ = writeln!(output, "  {line}");
                }
            }
            output.push('\n');
        }

        let _ = write!(output, "  {} facts  {} rules\n\n", facts.len(), rules.len());

        if !facts.is_empty() {
            output.push_str(&self.subsection_header("Facts"));
            output.push('\n');

            let max_name_len = facts
                .iter()
                .map(|f| lemma::analysis::fact_display_name(f).len())
                .max()
                .unwrap_or(0);

            for fact in facts {
                let name = lemma::analysis::fact_display_name(fact);
                let value_str = fact.value.to_string();

                let display = if self.use_colors {
                    match &fact.value {
                        lemma::FactValue::TypeAnnotation(_) => value_str.dark_grey().to_string(),
                        _ => value_str.green().to_string(),
                    }
                } else {
                    value_str
                };

                if self.use_colors {
                    let _ = writeln!(
                        output,
                        "  {:<width$}  {}",
                        name.bold(),
                        display,
                        width = max_name_len
                    );
                } else {
                    let _ = writeln!(output, "  {name:<max_name_len$}  {display}");
                }
            }
            output.push('\n');
        }

        if !rules.is_empty() {
            output.push_str(&self.subsection_header("Available Rules"));
            output.push('\n');

            let cols = 3;
            let rows = rules.len().div_ceil(cols);

            for row in 0..rows {
                let mut line = String::from("  ");
                for col in 0..cols {
                    let idx = row + col * rows;
                    if idx < rules.len() {
                        let name = &rules[idx].name;
                        if self.use_colors {
                            let _ = write!(line, "{:<30}", name.as_str().dark_grey());
                        } else {
                            let _ = write!(line, "{name:<30}");
                        }
                    }
                }
                output.push_str(line.trim_end());
                output.push('\n');
            }
        }

        output
    }

    pub fn format_workspace_summary(
        &self,
        file_count: usize,
        doc_count: usize,
        documents: &[(String, usize, usize)],
    ) -> String {
        let mut output = String::default();

        output.push_str(&self.section_divider());
        output.push_str(&self.style_header("  Workspace Summary"));
        output.push('\n');
        output.push_str(&self.section_divider());
        output.push('\n');

        output.push_str(&format!("  {file_count} files  {doc_count} documents\n\n"));

        if !documents.is_empty() {
            output.push_str(&self.subsection_header("Documents"));
            output.push('\n');

            let max_name_len = documents
                .iter()
                .map(|(name, _, _)| name.len())
                .max()
                .unwrap_or(0);

            for (name, facts, rules) in documents {
                let stats = format!("{facts} facts, {rules} rules");

                if self.use_colors {
                    output.push_str(&format!(
                        "  {:<width$}  {}\n",
                        name.as_str().bold(),
                        stats.dark_grey(),
                        width = max_name_len
                    ));
                } else {
                    output.push_str(&format!("  {name:<max_name_len$}  {stats}\n"));
                }
            }
        }

        output
    }

    fn section_divider(&self) -> String {
        if self.use_colors {
            format!("{}\n", "─".repeat(80).dark_grey())
        } else {
            format!("{}\n", "─".repeat(80))
        }
    }

    fn style_header(&self, text: &str) -> String {
        if self.use_colors {
            format!("{}\n", text.cyan().bold())
        } else {
            format!("{text}\n")
        }
    }

    fn subsection_header(&self, text: &str) -> String {
        if self.use_colors {
            format!("  {}\n", text.bold())
        } else {
            format!("  {text}\n")
        }
    }
}
