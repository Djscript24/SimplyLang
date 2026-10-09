//! formatter.rs — AST-based Simply source formatter.
use std::collections::{HashMap, HashSet, VecDeque};

use crate::{
    ast::{
        BinaryOperator, CatchClause, CollectionOperation, Expr, Literal, MatchArm, MatchPattern,
        PipelineStep, Program, Stmt, UnaryOperator,
    },
    lexer::Lexer,
    parser::Parser,
    types::Type,
};

pub fn format(source: &str) -> String {
    let parsed = Lexer::new(source)
        .tokenize()
        .and_then(|tokens| Parser::new(tokens).parse());
    let Ok(program) = parsed else {
        return source.to_owned();
    };
    Formatter::new(source).program(&program)
}

struct Formatter {
    output: String,
    source_lines: Vec<String>,
    comments: HashMap<usize, String>,
    match_arm_comments: HashMap<String, VecDeque<(usize, String)>>,
    emitted_comments: HashSet<usize>,
    end_lines: VecDeque<usize>,
    source_line: usize,
    active_line: Option<usize>,
}

impl Formatter {
    fn new(source: &str) -> Self {
        let mut comments = HashMap::new();
        let mut match_arm_comments = HashMap::<String, VecDeque<(usize, String)>>::new();
        let mut end_lines = VecDeque::new();
        let mut block_stack = Vec::new();
        let mut match_depths = 0usize;
        for (index, raw_line) in source.lines().enumerate() {
            let line = index + 1;
            let (code, comment) = split_code_comment(raw_line);
            if let Some(comment) = comment {
                comments.insert(line, comment.to_owned());
            }
            let trimmed = code.trim();
            if trimmed == "end" {
                if block_stack.pop().unwrap_or(false) {
                    match_depths = match_depths.saturating_sub(1);
                }
            } else if trimmed.starts_with("match ") || trimmed.contains(" is match ") {
                match_depths += 1;
                block_stack.push(true);
            } else {
                if let Some(comment) = comment
                    && trimmed.ends_with(':')
                    && match_depths > 0
                    && !is_match_arm_block_header(trimmed)
                {
                    match_arm_comments
                        .entry(compact_code(trimmed))
                        .or_default()
                        .push_back((line, comment.to_owned()));
                }
                if is_nested_block_header(trimmed) {
                    block_stack.push(false);
                }
            }
            if code.trim() == "end" {
                end_lines.push_back(line);
            }
        }
        Self {
            output: String::new(),
            source_lines: source.lines().map(str::to_owned).collect(),
            comments,
            match_arm_comments,
            emitted_comments: HashSet::new(),
            end_lines,
            source_line: 0,
            active_line: None,
        }
    }

    fn program(mut self, program: &Program) -> String {
        self.statements(&program.statements, 0);
        self.flush_comments(usize::MAX, 0);
        if !self.output.is_empty() && !self.output.ends_with('\n') {
            self.output.push('\n');
        }
        if self.output.ends_with('\n')
            && self.source_lines.last().is_some_and(|line| line.is_empty())
            && !self.output.ends_with("\n\n")
        {
            self.output.push('\n');
        }
        self.output
    }

    fn statements(&mut self, statements: &[Stmt], indent: usize) {
        for statement in statements {
            self.statement(statement, indent);
        }
    }

    fn statement(&mut self, statement: &Stmt, indent: usize) {
        if let Stmt::Located { span, statement } = statement {
            self.flush_comments(span.line, indent);
            self.source_line = span.line;
            self.active_line = Some(span.line);
            self.statement(statement, indent);
            return;
        }

        match statement {
            Stmt::Located { .. } => unreachable!(),
            Stmt::Say(value) => self.line(&format!("Say {}", expr(value)), indent, None),
            Stmt::Sayln(value) => self.line(&format!("Sayln {}", expr(value)), indent, None),
            Stmt::Expression(value) => self.expression_line(value, indent),
            Stmt::Import {
                path,
                alias: Some(alias),
                ..
            } => self.line(
                &format!("open {} as {alias}", string_literal(path)),
                indent,
                None,
            ),
            Stmt::Import {
                path,
                exposing,
                alias: None,
            } => self.line(
                &format!(
                    "open {} exposing {}",
                    string_literal(path),
                    format_named_imports(exposing)
                ),
                indent,
                None,
            ),
            Stmt::Export { names } => {
                self.line(&format!("export {}", names.join(", ")), indent, None)
            }
            Stmt::Assign {
                name,
                mutable,
                declared_type,
                value,
            } => {
                let mut prefix = String::new();
                if *mutable {
                    prefix.push_str("mut ");
                }
                prefix.push_str(name);
                if let Some(typ) = declared_type {
                    prefix.push_str(" as ");
                    prefix.push_str(&type_name(typ));
                }
                self.expression_line(
                    &Expr::Identifier(format!("{prefix} is {}", expr(value))),
                    indent,
                );
            }
            Stmt::Borrow {
                name,
                mutable,
                value,
            } => {
                self.expression_line(
                    &Expr::Identifier(format!(
                        "{}{name} is ref {}",
                        if *mutable { "mut " } else { "" },
                        expr(value)
                    )),
                    indent,
                );
            }
            Stmt::Flow {
                name,
                source,
                steps,
            } => {
                self.line(&format!("flow {name} from {}:", expr(source)), indent, None);
                self.flow_steps(steps, indent + 1);
                self.end(indent);
            }
            Stmt::Reassign { name, value } => self.expression_line(
                &Expr::Identifier(format!("{name} -> {}", expr(value))),
                indent,
            ),
            Stmt::DestructureReassign { pattern, value } => self.expression_line(
                &Expr::Identifier(format!("{} -> {}", render_pattern(pattern), expr(value))),
                indent,
            ),
            Stmt::SetIndex {
                name,
                indices,
                value,
            } => self.expression_line(
                &Expr::Identifier(format!(
                    "{name}{} is {}",
                    indices
                        .iter()
                        .map(|index| format!("[{}]", expr(index)))
                        .collect::<String>(),
                    expr(value)
                )),
                indent,
            ),
            Stmt::Destructure {
                pattern,
                mutable,
                value,
            } => {
                let prefix = if *mutable { "mut " } else { "" };
                self.expression_line(
                    &Expr::Identifier(format!(
                        "{prefix}{} is {}",
                        render_pattern(pattern),
                        expr(value)
                    )),
                    indent,
                );
            }
            Stmt::CollectionOp {
                name,
                operation,
                value,
            } => {
                let operation = match operation {
                    CollectionOperation::Add => "add",
                    CollectionOperation::Remove => "remove",
                };
                self.expression_line(
                    &Expr::Identifier(format!("{name} {operation} {}", expr(value))),
                    indent,
                );
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.line(&format!("if {}:", expr(condition)), indent, None);
                self.statements(then_branch, indent + 1);
                if let [
                    Stmt::If {
                        condition,
                        then_branch,
                        else_branch,
                    },
                ] = else_branch.as_slice()
                {
                    self.line(&format!("else if {}:", expr(condition)), indent, None);
                    self.statements(then_branch, indent + 1);
                    if !else_branch.is_empty() {
                        self.line("else:", indent, None);
                        self.statements(else_branch, indent + 1);
                    }
                } else if !else_branch.is_empty() {
                    self.line("else:", indent, None);
                    self.statements(else_branch, indent + 1);
                }
                self.end(indent);
            }
            Stmt::Throw(value) => {
                self.expression_line(&Expr::Identifier(format!("throw {}", expr(value))), indent)
            }
            Stmt::Try {
                try_body,
                catches,
                finally_body,
            } => {
                self.line("try:", indent, None);
                self.statements(try_body, indent + 1);
                for catch in catches {
                    self.catch_clause(catch, indent);
                }
                if !finally_body.is_empty() {
                    self.line("finally:", indent, None);
                    self.statements(finally_body, indent + 1);
                }
                self.end(indent);
            }
            Stmt::Function {
                name,
                parameters,
                return_type,
                body,
            } => {
                let mut header = format!("fn {name}({})", function_parameters_text(parameters));
                if let Some(typ) = return_type {
                    header.push_str(" gives ");
                    header.push_str(&type_name(typ));
                }
                header.push(':');
                self.line(&header, indent, None);
                self.statements(body, indent + 1);
                self.end(indent);
            }
            Stmt::Struct { name, fields } => {
                self.line(&format!("type {name}:"), indent, None);
                for field in fields {
                    self.line(
                        &format!("{} as {}", field.name, type_name(&field.field_type)),
                        indent + 1,
                        None,
                    );
                }
                self.end(indent);
            }
            Stmt::Enum { name, variants } => {
                self.line(&format!("enum {name}:"), indent, None);
                for variant in variants {
                    let suffix = variant
                        .payload_type
                        .as_ref()
                        .map(|typ| format!(" as {}", type_name(typ)))
                        .unwrap_or_default();
                    self.line(&format!("{}{suffix}", variant.name), indent + 1, None);
                }
                self.end(indent);
            }
            Stmt::Message {
                receiver_type,
                name,
                parameters,
                body,
            } => {
                self.line(
                    &format!(
                        "on {receiver_type} receive {name}({}):",
                        function_parameters_text(parameters)
                    ),
                    indent,
                    None,
                );
                self.statements(body, indent + 1);
                self.end(indent);
            }
            Stmt::Return(value) => {
                self.expression_line(&Expr::Identifier(format!("return {}", expr(value))), indent)
            }
            Stmt::For {
                name,
                mutable,
                by_ref,
                iterable,
                body,
            } => {
                let binding = if *by_ref {
                    "ref "
                } else if *mutable {
                    "mut "
                } else {
                    ""
                };
                self.line(
                    &format!("for {binding}{name} in {}:", expr(iterable)),
                    indent,
                    None,
                );
                self.statements(body, indent + 1);
                self.end(indent);
            }
            Stmt::While { condition, body } => {
                self.line(&format!("while {}:", expr(condition)), indent, None);
                self.statements(body, indent + 1);
                self.end(indent);
            }
            Stmt::Break => self.line("break", indent, None),
            Stmt::Continue => self.line("continue", indent, None),
        }
    }

    fn catch_clause(&mut self, catch: &CatchClause, indent: usize) {
        let mut header = "catch".to_owned();
        if let Some(binding) = &catch.binding {
            header.push(' ');
            header.push_str(binding);
        }
        if let Some(code) = &catch.code {
            header.push_str(" as ");
            header.push_str(code);
        }
        header.push(':');
        self.line(&header, indent, None);
        self.statements(&catch.body, indent + 1);
    }

    fn flow_steps(&mut self, steps: &[PipelineStep], indent: usize) {
        for step in steps {
            match step {
                PipelineStep::Where(value) => self
                    .expression_line(&Expr::Identifier(format!("where {}", expr(value))), indent),
                PipelineStep::Derive(value) => self
                    .expression_line(&Expr::Identifier(format!("derive {}", expr(value))), indent),
                PipelineStep::Take(count) => self.line(&format!("take {count}"), indent, None),
                PipelineStep::Skip(count) => self.line(&format!("skip {count}"), indent, None),
                PipelineStep::StepBy(interval) => {
                    self.line(&format!("step_by {interval}"), indent, None)
                }
                PipelineStep::TakeWhile(value) => self.expression_line(
                    &Expr::Identifier(format!("take_while {}", expr(value))),
                    indent,
                ),
                PipelineStep::DropWhile(value) => self.expression_line(
                    &Expr::Identifier(format!("drop_while {}", expr(value))),
                    indent,
                ),
                PipelineStep::Distinct => self.line("distinct", indent, None),
                PipelineStep::Partition { item, rules } => {
                    self.line(&format!("partition {item}:"), indent, None);
                    for rule in rules {
                        let condition = rule
                            .condition
                            .as_ref()
                            .map(expr)
                            .unwrap_or_else(|| "otherwise".into());
                        self.line(
                            &format!("{condition} -> {}", rule.category),
                            indent + 1,
                            None,
                        );
                    }
                    self.end(indent);
                }
                PipelineStep::Sum => self.line("sum", indent, None),
                PipelineStep::Count => self.line("count", indent, None),
                PipelineStep::Average => self.line("average", indent, None),
                PipelineStep::Min => self.line("min", indent, None),
                PipelineStep::Max => self.line("max", indent, None),
                PipelineStep::Any => self.line("any", indent, None),
                PipelineStep::All => self.line("all", indent, None),
                PipelineStep::WriteCsv(path) => {
                    self.line(&format!("write_csv({})", expr(path)), indent, None)
                }
                PipelineStep::Chunk(size) => self.line(&format!("chunk {size}"), indent, None),
                PipelineStep::Parallel(workers) => {
                    self.line(&format!("parallel {workers}"), indent, None)
                }
                PipelineStep::Checkpoint(path) => {
                    self.line(&format!("checkpoint {}", expr(path)), indent, None)
                }
            }
        }
    }

    fn expression_line(&mut self, expression: &Expr, indent: usize) {
        self.text_line(&expr(expression), indent, None);
    }

    fn end(&mut self, indent: usize) {
        if let Some(line) = self.end_lines.pop_front() {
            self.flush_comments(line, indent);
            self.source_line = line;
            self.line("end", indent, Some(line));
        } else {
            self.line("end", indent, None);
        }
    }

    fn flush_comments(&mut self, before_line: usize, indent: usize) {
        let mut lines = self
            .comments
            .keys()
            .copied()
            .filter(|line| {
                *line > self.source_line
                    && *line < before_line
                    && !self.emitted_comments.contains(line)
            })
            .collect::<Vec<_>>();
        lines.sort_unstable();
        for line in lines {
            let comment = self.comments[&line].clone();
            self.line(&comment, indent, None);
            self.emitted_comments.insert(line);
            self.source_line = line;
        }
    }

    fn line(&mut self, text: &str, indent: usize, source_line: Option<usize>) {
        self.text_line(text, indent, source_line);
    }

    fn text_line(&mut self, text: &str, indent: usize, source_line: Option<usize>) {
        let padding = "    ".repeat(indent);
        let mut lines = text.split('\n');
        if let Some(first) = lines.next() {
            self.output.push_str(&padding);
            self.output.push_str(first);
            let active_line = self.active_line.take();
            if let Some(line) = source_line.or(active_line)
                && let Some(comment) = self.comments.get(&line)
            {
                self.output.push(' ');
                self.output.push_str(comment);
                self.emitted_comments.insert(line);
            }
            self.output.push('\n');
        } else {
            self.active_line = None;
        }
        for line in lines {
            let close_line = line
                .trim_start()
                .strip_prefix("end")
                .filter(|suffix| {
                    suffix.is_empty()
                        || suffix.starts_with(')')
                        || suffix.starts_with(' ')
                        || suffix.starts_with('\t')
                })
                .and_then(|_| self.end_lines.pop_front());
            if let Some(close_line) = close_line {
                let close_indent = line.len() - line.trim_start().len();
                self.flush_comments(close_line, indent + close_indent / 4);
                self.source_line = close_line;
            }
            self.output.push_str(&padding);
            self.output.push_str(line);
            if line.trim_end().ends_with(':')
                && let Some((comment_line, comment)) = self
                    .match_arm_comments
                    .get_mut(&compact_code(line.trim()))
                    .and_then(VecDeque::pop_front)
            {
                self.output.push(' ');
                self.output.push_str(&comment);
                self.emitted_comments.insert(comment_line);
            }
            if let Some(close_line) = close_line
                && let Some(comment) = self.comments.get(&close_line)
            {
                self.output.push(' ');
                self.output.push_str(comment);
                self.emitted_comments.insert(close_line);
            }
            self.output.push('\n');
        }
    }
}

fn split_code_comment(line: &str) -> (&str, Option<&str>) {
    let mut in_string = false;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if character == '#' && !in_string {
            return (&line[..index], Some(&line[index..]));
        }
        if character == '"' && !escaped {
            in_string = !in_string;
        }
        escaped = character == '\\' && !escaped;
        if character != '\\' {
            escaped = false;
        }
    }
    (line, None)
}

fn is_match_arm_block_header(line: &str) -> bool {
    [
        "if ",
        "else",
        "for ",
        "while ",
        "try:",
        "catch",
        "finally:",
        "fn ",
        "type ",
        "enum ",
        "hash:",
        "pipeline:",
        "flow ",
        "partition ",
    ]
    .iter()
    .any(|prefix| line.starts_with(prefix))
}

fn is_nested_block_header(line: &str) -> bool {
    line.ends_with(':')
        && [
            "if ",
            "for ",
            "while ",
            "try:",
            "fn ",
            "type ",
            "enum ",
            "hash:",
            "pipeline:",
            "flow ",
            "partition ",
        ]
        .iter()
        .any(|prefix| line.starts_with(prefix))
}

fn compact_code(line: &str) -> String {
    let mut compact = String::new();
    let mut in_string = false;
    let mut escaped = false;
    for character in line.chars() {
        if !in_string && character.is_whitespace() {
            continue;
        }
        compact.push(character);
        if character == '"' && !escaped {
            in_string = !in_string;
        }
        escaped = character == '\\' && !escaped;
        if character != '\\' {
            escaped = false;
        }
    }
    compact
}

fn function_parameters_text(parameters: &[(String, Option<Type>, bool, bool)]) -> String {
    parameters
        .iter()
        .map(|(name, typ, mutable, by_ref)| {
            let mut text = if *by_ref && *mutable {
                format!("mut ref {name}")
            } else if *by_ref {
                format!("ref {name}")
            } else if *mutable {
                format!("mut {name}")
            } else {
                name.clone()
            };
            if let Some(typ) = typ {
                text.push_str(" as ");
                text.push_str(&type_name(typ));
            }
            text
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn type_name(typ: &Type) -> String {
    match typ {
        Type::Array(inner) => format!("Array[{}]", type_name(inner)),
        Type::List(inner) => format!("List[{}]", type_name(inner)),
        Type::Tuple(elements) => format!(
            "({})",
            elements
                .iter()
                .map(type_name)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => typ.name(),
    }
}

fn string_literal(value: &str) -> String {
    let mut output = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\t' => output.push_str("\\t"),
            '\r' => output.push_str("\\r"),
            other => output.push(other),
        }
    }
    output.push('"');
    output
}

fn literal(value: &Literal) -> String {
    match value {
        Literal::String(value) => string_literal(value),
        Literal::Int(value) => value.to_string(),
        Literal::Float(value) => {
            let rendered = value.to_string();
            if rendered.contains('.') || rendered.contains('e') || rendered.contains('E') {
                rendered
            } else {
                format!("{rendered}.0")
            }
        }
        Literal::Bool(value) => value.to_string(),
    }
}

fn precedence(operator: &BinaryOperator) -> u8 {
    match operator {
        BinaryOperator::Or => 1,
        BinaryOperator::And => 2,
        BinaryOperator::Equal | BinaryOperator::NotEqual => 3,
        BinaryOperator::In => 5,
        BinaryOperator::Greater
        | BinaryOperator::GreaterEqual
        | BinaryOperator::Less
        | BinaryOperator::LessEqual => 5,
        BinaryOperator::Range => 6,
        BinaryOperator::Add | BinaryOperator::Subtract | BinaryOperator::MatrixMultiply => 7,
        BinaryOperator::Multiply | BinaryOperator::Divide | BinaryOperator::Remainder => 8,
    }
}

fn binary_symbol(operator: &BinaryOperator) -> &'static str {
    match operator {
        BinaryOperator::And => "and",
        BinaryOperator::Or => "or",
        BinaryOperator::Add => "+",
        BinaryOperator::Subtract => "-",
        BinaryOperator::Multiply => "*",
        BinaryOperator::Divide => "/",
        BinaryOperator::Remainder => "%",
        BinaryOperator::MatrixMultiply => "multiply",
        BinaryOperator::Greater => ">",
        BinaryOperator::GreaterEqual => ">=",
        BinaryOperator::Less => "<",
        BinaryOperator::LessEqual => "<=",
        BinaryOperator::In => "in",
        BinaryOperator::Range => "..",
        BinaryOperator::Equal => "==",
        BinaryOperator::NotEqual => "!=",
    }
}

fn format_named_imports(imports: &[(String, String)]) -> String {
    imports
        .iter()
        .map(|(exported, local)| {
            if exported == local {
                exported.clone()
            } else {
                format!("{exported} as {local}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn expr(expression: &Expr) -> String {
    expr_prec(expression, 0)
}

fn expr_prec(expression: &Expr, parent: u8) -> String {
    let (text, own) = match expression {
        Expr::Literal(value) => (literal(value), 10),
        Expr::Identifier(name) => (name.clone(), 10),
        Expr::Unary { operator, operand } => {
            let (symbol, precedence) = match operator {
                UnaryOperator::Not => ("not ", 9),
                UnaryOperator::Negate => ("-", 9),
                UnaryOperator::Transpose => ("", 10),
            };
            (
                format!(
                    "{symbol}{}{}",
                    expr_prec(operand, precedence),
                    if matches!(operator, UnaryOperator::Transpose) {
                        " transpose"
                    } else {
                        ""
                    }
                ),
                precedence,
            )
        }
        Expr::Binary {
            left,
            operator,
            right,
        } => {
            let p = precedence(operator);
            (
                format!(
                    "{} {} {}",
                    expr_prec(left, p),
                    binary_symbol(operator),
                    expr_prec(right, p + 1)
                ),
                p,
            )
        }
        Expr::Call { name, arguments } => (
            format!(
                "{name}({})",
                arguments.iter().map(expr).collect::<Vec<_>>().join(", ")
            ),
            9,
        ),
        Expr::MessageDispatch {
            receiver,
            message,
            arguments,
        } => (
            format!(
                "{} :: {message}{}",
                expr_prec(receiver, 9),
                arguments_suffix(arguments)
            ),
            9,
        ),
        Expr::EnumVariant {
            enum_name,
            variant_name,
            arguments,
        } => (
            format!("{enum_name}::{variant_name}{}", arguments_suffix(arguments)),
            9,
        ),
        Expr::Array(values) => (
            format!(
                "[{}]",
                values.iter().map(expr).collect::<Vec<_>>().join(", ")
            ),
            9,
        ),
        Expr::List(values) => (
            format!(
                "list [{}]",
                values.iter().map(expr).collect::<Vec<_>>().join(", ")
            ),
            9,
        ),
        Expr::Tuple(values) => (
            format!(
                "({})",
                values.iter().map(expr).collect::<Vec<_>>().join(", ")
            ),
            9,
        ),
        Expr::Index { target, index } => (format!("{}[{}]", expr_prec(target, 9), expr(index)), 9),
        Expr::Field { target, name } => (format!("{}.{}", expr_prec(target, 9), name), 9),
        Expr::Hash(values) => {
            let mut text = String::from("hash:");
            for (name, value) in values {
                text.push_str(&format!("\n    {name} is {}", expr(value)));
            }
            text.push_str("\nend");
            (text, 9)
        }
        Expr::Matrix(values) => (
            format!(
                "matrix[{}]",
                values.iter().map(expr).collect::<Vec<_>>().join(", ")
            ),
            9,
        ),
        Expr::Pipeline { source, steps } => (pipeline_expr(source, steps), 9),
        Expr::Match { value, arms } => (match_expr(value, arms), 9),
    };
    if own < parent {
        format!("({text})")
    } else {
        text
    }
}

fn arguments_suffix(arguments: &[Expr]) -> String {
    if arguments.is_empty() {
        String::new()
    } else {
        format!(
            "({})",
            arguments.iter().map(expr).collect::<Vec<_>>().join(", ")
        )
    }
}

fn pipeline_expr(source: &Expr, steps: &[PipelineStep]) -> String {
    let mut lines = vec!["pipeline:".to_owned(), format!("    {}", expr(source))];
    for step in steps {
        match step {
            PipelineStep::Where(value) => lines.push(format!("    where {}", expr(value))),
            PipelineStep::Derive(value) => lines.push(format!("    derive {}", expr(value))),
            PipelineStep::Take(count) => lines.push(format!("    take {count}")),
            PipelineStep::Skip(count) => lines.push(format!("    skip {count}")),
            PipelineStep::StepBy(interval) => lines.push(format!("    step_by {interval}")),
            PipelineStep::TakeWhile(value) => lines.push(format!("    take_while {}", expr(value))),
            PipelineStep::DropWhile(value) => lines.push(format!("    drop_while {}", expr(value))),
            PipelineStep::Distinct => lines.push("    distinct".into()),
            PipelineStep::Partition { item, rules } => {
                lines.push(format!("    partition {item}:"));
                lines.extend(rules.iter().map(|rule| {
                    format!(
                        "        {} -> {}",
                        rule.condition
                            .as_ref()
                            .map(expr)
                            .unwrap_or_else(|| "otherwise".into()),
                        rule.category
                    )
                }));
                lines.push("    end".into());
            }
            PipelineStep::Sum => lines.push("    sum".into()),
            PipelineStep::Count => lines.push("    count".into()),
            PipelineStep::Average => lines.push("    average".into()),
            PipelineStep::Min => lines.push("    min".into()),
            PipelineStep::Max => lines.push("    max".into()),
            PipelineStep::Any => lines.push("    any".into()),
            PipelineStep::All => lines.push("    all".into()),
            PipelineStep::WriteCsv(path) => lines.push(format!("    write_csv({})", expr(path))),
            PipelineStep::Chunk(size) => lines.push(format!("    chunk {size}")),
            PipelineStep::Parallel(workers) => lines.push(format!("    parallel {workers}")),
            PipelineStep::Checkpoint(path) => lines.push(format!("    checkpoint {}", expr(path))),
        }
    }
    lines.push("end".into());
    lines.join("\n")
}

fn match_expr(value: &Expr, arms: &[MatchArm]) -> String {
    let mut lines = vec![format!("match {}:", expr(value))];
    for arm in arms {
        let mut header = format!("    {}", render_pattern(&arm.pattern));
        if let Some(guard) = &arm.guard {
            header.push_str(&format!(" if {}", expr(guard)));
        }
        header.push(':');
        lines.push(header);
        for statement in &arm.body {
            lines.extend(
                indent_text(&statement_text(statement), 8)
                    .lines()
                    .map(str::to_owned),
            );
        }
        if let Some(result) = &arm.result {
            lines.push(format!("        {}", expr(result)));
        }
    }
    lines.push("end".into());
    lines.join("\n")
}

fn statement_text(statement: &Stmt) -> String {
    match statement {
        Stmt::Located { statement, .. } => statement_text(statement),
        Stmt::Say(value) => format!("Say {}", expr(value)),
        Stmt::Sayln(value) => format!("Sayln {}", expr(value)),
        Stmt::Expression(value) => expr(value),
        Stmt::Import {
            path,
            alias: Some(alias),
            ..
        } => format!("open {} as {alias}", string_literal(path)),
        Stmt::Import {
            path,
            exposing,
            alias: None,
        } => format!(
            "open {} exposing {}",
            string_literal(path),
            format_named_imports(exposing)
        ),
        Stmt::Export { names } => format!("export {}", names.join(", ")),
        Stmt::Assign {
            name,
            mutable,
            declared_type,
            value,
        } => {
            let prefix = if *mutable { "mut " } else { "" };
            let declared_type = declared_type
                .as_ref()
                .map(|typ| format!(" as {}", type_name(typ)))
                .unwrap_or_default();
            format!("{prefix}{name}{declared_type} is {}", expr(value))
        }
        Stmt::Borrow {
            name,
            mutable,
            value,
        } => format!(
            "{}{name} is ref {}",
            if *mutable { "mut " } else { "" },
            expr(value)
        ),
        Stmt::Flow {
            name,
            source,
            steps,
        } => {
            let mut lines = vec![format!("flow {name} from {}:", expr(source))];
            lines.extend(render_steps(steps, 4));
            lines.push("end".into());
            lines.join("\n")
        }
        Stmt::Reassign { name, value } => format!("{name} -> {}", expr(value)),
        Stmt::DestructureReassign { pattern, value } => {
            format!("{} -> {}", render_pattern(pattern), expr(value))
        }
        Stmt::SetIndex {
            name,
            indices,
            value,
        } => {
            format!(
                "{name}{} is {}",
                indices
                    .iter()
                    .map(|index| format!("[{}]", expr(index)))
                    .collect::<String>(),
                expr(value)
            )
        }
        Stmt::Destructure {
            pattern,
            mutable,
            value,
        } => format!(
            "{}{} is {}",
            if *mutable { "mut " } else { "" },
            render_pattern(pattern),
            expr(value)
        ),
        Stmt::CollectionOp {
            name,
            operation,
            value,
        } => format!(
            "{name} {} {}",
            match operation {
                CollectionOperation::Add => "add",
                CollectionOperation::Remove => "remove",
            },
            expr(value)
        ),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => render_if(condition, then_branch, else_branch),
        Stmt::Return(value) => format!("return {}", expr(value)),
        Stmt::Throw(value) => format!("throw {}", expr(value)),
        Stmt::Try {
            try_body,
            catches,
            finally_body,
        } => {
            let mut lines = vec!["try:".to_owned()];
            lines.extend(render_body(try_body, 4));
            for catch in catches {
                let mut header = "catch".to_owned();
                if let Some(binding) = &catch.binding {
                    header.push_str(&format!(" {binding}"));
                }
                if let Some(code) = &catch.code {
                    header.push_str(&format!(" as {code}"));
                }
                lines.push(format!("{header}:"));
                lines.extend(render_body(&catch.body, 4));
            }
            if !finally_body.is_empty() {
                lines.push("finally:".into());
                lines.extend(render_body(finally_body, 4));
            }
            lines.push("end".into());
            lines.join("\n")
        }
        Stmt::Function {
            name,
            parameters,
            return_type,
            body,
        } => {
            let mut header = format!("fn {name}({})", function_parameters_text(parameters));
            if let Some(typ) = return_type {
                header.push_str(&format!(" gives {}", type_name(typ)));
            }
            header.push(':');
            render_block(header, body)
        }
        Stmt::Struct { name, fields } => {
            let mut lines = vec![format!("type {name}:")];
            lines.extend(
                fields
                    .iter()
                    .map(|field| format!("    {} as {}", field.name, type_name(&field.field_type))),
            );
            lines.push("end".into());
            lines.join("\n")
        }
        Stmt::Enum { name, variants } => {
            let mut lines = vec![format!("enum {name}:")];
            lines.extend(variants.iter().map(|variant| {
                format!(
                    "    {}{}",
                    variant.name,
                    variant
                        .payload_type
                        .as_ref()
                        .map(|typ| format!(" as {}", type_name(typ)))
                        .unwrap_or_default()
                )
            }));
            lines.push("end".into());
            lines.join("\n")
        }
        Stmt::Message {
            receiver_type,
            name,
            parameters,
            body,
        } => render_block(
            format!(
                "on {receiver_type} receive {name}({}):",
                function_parameters_text(parameters)
            ),
            body,
        ),
        Stmt::For {
            name,
            mutable,
            by_ref,
            iterable,
            body,
        } => render_block(
            format!(
                "for {}{name} in {}:",
                if *by_ref {
                    "ref "
                } else if *mutable {
                    "mut "
                } else {
                    ""
                },
                expr(iterable)
            ),
            body,
        ),
        Stmt::While { condition, body } => {
            render_block(format!("while {}:", expr(condition)), body)
        }
        Stmt::Break => "break".into(),
        Stmt::Continue => "continue".into(),
    }
}

fn render_if(condition: &Expr, then_branch: &[Stmt], else_branch: &[Stmt]) -> String {
    let mut lines = vec![format!("if {}:", expr(condition))];
    lines.extend(render_body(then_branch, 4));
    if let [
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        },
    ] = else_branch
    {
        lines.push(format!("else if {}:", expr(condition)));
        lines.extend(render_body(then_branch, 4));
        if !else_branch.is_empty() {
            lines.push("else:".into());
            lines.extend(render_body(else_branch, 4));
        }
    } else if !else_branch.is_empty() {
        lines.push("else:".into());
        lines.extend(render_body(else_branch, 4));
    }
    lines.push("end".into());
    lines.join("\n")
}

fn render_block(header: String, body: &[Stmt]) -> String {
    let mut lines = vec![header];
    lines.extend(render_body(body, 4));
    lines.push("end".into());
    lines.join("\n")
}

fn render_body(body: &[Stmt], indent: usize) -> Vec<String> {
    body.iter()
        .flat_map(|statement| {
            indent_text(&statement_text(statement), indent)
                .lines()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn indent_text(text: &str, indent: usize) -> String {
    let padding = " ".repeat(indent);
    text.lines()
        .map(|line| format!("{padding}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_steps(steps: &[PipelineStep], indent: usize) -> Vec<String> {
    let pad = " ".repeat(indent);
    let mut lines = Vec::new();
    for step in steps {
        match step {
            PipelineStep::Where(value) => lines.push(format!("{pad}where {}", expr(value))),
            PipelineStep::Derive(value) => lines.push(format!("{pad}derive {}", expr(value))),
            PipelineStep::Take(count) => lines.push(format!("{pad}take {count}")),
            PipelineStep::Skip(count) => lines.push(format!("{pad}skip {count}")),
            PipelineStep::StepBy(interval) => lines.push(format!("{pad}step_by {interval}")),
            PipelineStep::TakeWhile(value) => {
                lines.push(format!("{pad}take_while {}", expr(value)))
            }
            PipelineStep::DropWhile(value) => {
                lines.push(format!("{pad}drop_while {}", expr(value)))
            }
            PipelineStep::Distinct => lines.push(format!("{pad}distinct")),
            PipelineStep::Partition { item, rules } => {
                lines.push(format!("{pad}partition {item}:"));
                for rule in rules {
                    lines.push(format!(
                        "{pad}    {} -> {}",
                        rule.condition
                            .as_ref()
                            .map(expr)
                            .unwrap_or_else(|| "otherwise".into()),
                        rule.category
                    ));
                }
                lines.push(format!("{pad}end"));
            }
            PipelineStep::Sum => lines.push(format!("{pad}sum")),
            PipelineStep::Count => lines.push(format!("{pad}count")),
            PipelineStep::Average => lines.push(format!("{pad}average")),
            PipelineStep::Min => lines.push(format!("{pad}min")),
            PipelineStep::Max => lines.push(format!("{pad}max")),
            PipelineStep::Any => lines.push(format!("{pad}any")),
            PipelineStep::All => lines.push(format!("{pad}all")),
            PipelineStep::WriteCsv(path) => lines.push(format!("{pad}write_csv({})", expr(path))),
            PipelineStep::Chunk(size) => lines.push(format!("{pad}chunk {size}")),
            PipelineStep::Parallel(workers) => lines.push(format!("{pad}parallel {workers}")),
            PipelineStep::Checkpoint(path) => lines.push(format!("{pad}checkpoint {}", expr(path))),
        }
    }
    lines
}

fn render_pattern(value: &MatchPattern) -> String {
    match value {
        MatchPattern::Identifier(name) => name.clone(),
        MatchPattern::ReferenceIdentifier(name) => format!("ref {name}"),
        MatchPattern::Literal(value) => literal(value),
        MatchPattern::Range {
            start,
            end,
            inclusive_end,
        } => format!(
            "{}{}{}",
            start.as_ref().map(literal).unwrap_or_default(),
            if *inclusive_end { "..=" } else { ".." },
            end.as_ref().map(literal).unwrap_or_default()
        ),
        MatchPattern::Or(values) => values
            .iter()
            .map(render_pattern)
            .collect::<Vec<_>>()
            .join(" | "),
        MatchPattern::Tuple(values) => {
            format!(
                "({})",
                values
                    .iter()
                    .map(render_pattern)
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        MatchPattern::Sequence { patterns, rest } => {
            let mut values = patterns.iter().map(render_pattern).collect::<Vec<_>>();
            if let Some(rest) = rest {
                values.push(format!("...{rest}"));
            }
            format!("[{}]", values.join(", "))
        }
        MatchPattern::Hash(values) => format!(
            "{{{}}}",
            values
                .iter()
                .map(|(key, value)| {
                    let key = if key
                        .chars()
                        .next()
                        .is_some_and(|ch| ch == '_' || ch.is_alphabetic())
                        && key.chars().all(|ch| ch == '_' || ch.is_alphanumeric())
                    {
                        key.clone()
                    } else {
                        string_literal(key)
                    };
                    format!("{key}: {}", render_pattern(value))
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        MatchPattern::Alias {
            name,
            pattern: inner,
        } => format!("{name} @ {}", render_pattern(inner)),
        MatchPattern::EnumVariant {
            enum_name,
            variant_name,
            payload,
        } => format!(
            "{enum_name}::{variant_name}{}",
            payload
                .as_ref()
                .map(|payload| format!("({})", render_pattern(payload)))
                .unwrap_or_default()
        ),
        MatchPattern::Struct { type_name, fields } => format!(
            "{type_name}({})",
            fields
                .iter()
                .map(render_pattern)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        MatchPattern::NamedStruct { type_name, fields } => format!(
            "{type_name}({})",
            fields
                .iter()
                .map(|(name, pattern)| format!("{name}: {}", render_pattern(pattern)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        MatchPattern::Wildcard => "_".into(),
    }
}
