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
            Stmt::SetIndex { name, index, value } => self.expression_line(
                &Expr::Identifier(format!("{name}[{}] is {}", expr(index), expr(value))),
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
                let mut header = format!("fn {name}({})", parameters_text(parameters));
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
                        parameters_text(parameters)
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
                iterable,
                body,
            } => {
                let mutable = if *mutable { "mut " } else { "" };
                self.line(
                    &format!("for {mutable}{name} in {}:", expr(iterable)),
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
        "tree:",
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
            "tree:",
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

fn parameters_text(parameters: &[(String, Option<Type>, bool)]) -> String {
    parameters
        .iter()
        .map(|(name, typ, mutable)| {
            let mut text = if *mutable {
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
        BinaryOperator::Greater
        | BinaryOperator::GreaterEqual
        | BinaryOperator::Less
        | BinaryOperator::LessEqual => 4,
        BinaryOperator::Add | BinaryOperator::Subtract | BinaryOperator::MatrixMultiply => 5,
        BinaryOperator::Multiply | BinaryOperator::Divide | BinaryOperator::Remainder => 6,
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
                UnaryOperator::Not => ("not ", 7),
                UnaryOperator::Negate => ("-", 7),
                UnaryOperator::Transpose => ("", 8),
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
        Expr::Tree(values) => {
            let mut text = String::from("tree:");
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
        Stmt::SetIndex { name, index, value } => {
            format!("{name}[{}] is {}", expr(index), expr(value))
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
            let mut header = format!("fn {name}({})", parameters_text(parameters));
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
                parameters_text(parameters)
            ),
            body,
        ),
        Stmt::For {
            name,
            mutable,
            iterable,
            body,
        } => render_block(
            format!(
                "for {}{name} in {}:",
                if *mutable { "mut " } else { "" },
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
        MatchPattern::Literal(value) => literal(value),
        MatchPattern::Range { start, end } => format!(
            "{}..{}",
            start.as_ref().map(literal).unwrap_or_default(),
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

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{format, statement_text};

    #[test]
    fn normalizes_block_indentation() {
        assert_eq!(
            format("if true:\nSay \"yes\"\nend\n"),
            "if true:\n    Say \"yes\"\nend\n"
        );
    }

    #[test]
    fn does_not_treat_text_as_a_block() {
        assert_eq!(
            format("Say \"elsewhere: # still text\"\n"),
            "Say \"elsewhere: # still text\"\n"
        );
    }

    #[test]
    fn formats_commented_block_closers() {
        assert_eq!(
            format("if true:\nSay \"yes\"\nend # done\n"),
            "if true:\n    Say \"yes\"\nend # done\n"
        );
    }

    #[test]
    fn is_idempotent_for_nested_blocks_and_blank_lines() {
        let source =
            "fn greet(name):\nif true:\nSay \"hello, \" + name\nelse:\nSay \"no\"\nend\nend\n\n";
        let formatted = format(source);

        assert_eq!(format(&formatted), formatted);
        assert_eq!(
            formatted,
            "fn greet(name):\n    if true:\n        Say \"hello, \" + name\n    else:\n        Say \"no\"\n    end\nend\n\n"
        );
    }

    #[test]
    fn preserves_string_contents_and_comment_text() {
        let source = "Say \"  # not a comment  \" # keep  \n#  keep trailing spaces  \n";

        assert_eq!(
            format(source),
            "Say \"  # not a comment  \" # keep  \n#  keep trailing spaces  \n"
        );
    }

    #[test]
    fn keeps_match_arm_header_comments_with_their_arms() {
        let source = "result is match 1:\n\
                      1: # first arm\n\
                      10\n\
                      _: # fallback arm\n\
                      0\n\
                      end\n\
                      Sayln result\n";
        let formatted = format(source);

        assert!(formatted.contains("    1: # first arm\n"), "{formatted}");
        assert!(formatted.contains("    _: # fallback arm\n"), "{formatted}");
        assert!(!formatted.contains("# first arm\nend"), "{formatted}");
        assert_eq!(format(&formatted), formatted);
    }

    #[test]
    fn formats_nested_same_line_expressions_from_the_ast() {
        let source = "answer is outer(inner([1,2]),(3+4)*5)\n\
                      values is list [1, 2, 3]\n\
                      selected is match values[0]:\n\
                      [head, ...tail]:\n\
                      Sayln nested(head, tail)\n\
                      head + tail[0]\n\
                      _:\n\
                      0\n\
                      end\n";
        let formatted = format(source);
        let parse = |source: &str| {
            crate::parser::Parser::new(crate::lexer::Lexer::new(source).tokenize().unwrap())
                .parse()
                .unwrap()
        };
        let original = parse(source);
        let parsed = parse(&formatted);

        fn selected_match(program: &crate::ast::Program) -> &[crate::ast::MatchArm] {
            let crate::ast::Stmt::Located { statement, .. } = &program.statements[2] else {
                panic!("expected located assignment");
            };
            let crate::ast::Stmt::Assign {
                value: crate::ast::Expr::Match { arms, .. },
                ..
            } = statement.as_ref()
            else {
                panic!("expected assignment to a match expression");
            };
            arms
        }
        let original_arms = selected_match(&original);
        let formatted_arms = selected_match(&parsed);

        assert_eq!(format(&formatted), formatted);
        assert_eq!(parsed.statements.len(), 3);
        assert_eq!(original_arms.len(), formatted_arms.len());
        for (original, formatted) in original_arms.iter().zip(formatted_arms) {
            assert_eq!(original.pattern, formatted.pattern);
            assert_eq!(original.guard, formatted.guard);
            assert_eq!(original.result, formatted.result);
            assert_eq!(original.body.len(), formatted.body.len());
            for (original, formatted) in original.body.iter().zip(&formatted.body) {
                assert_eq!(statement_text(original), statement_text(formatted));
            }
        }
        assert!(formatted_arms[0].result.is_some());
        assert!(formatted.contains("outer(inner([1, 2]), (3 + 4) * 5)"));
        assert!(formatted.contains("match values[0]:\n"));
    }

    #[test]
    fn formats_flow_steps_and_preserves_their_ast() {
        let source = "flow totals from [1,2,3]:\n\
                      where item>1\n\
                      derive item*2\n\
                      sum\n\
                      end\n";
        let formatted = format(source);
        let parse = |source: &str| {
            crate::parser::Parser::new(crate::lexer::Lexer::new(source).tokenize().unwrap())
                .parse()
                .unwrap()
        };

        assert_eq!(format(&formatted), formatted);
        assert_eq!(parse(source), parse(&formatted));
        assert_eq!(
            formatted,
            "flow totals from [1, 2, 3]:\n    where item > 1\n    derive item * 2\n    sum\nend\n"
        );
    }

    #[test]
    fn invalid_source_returns_unchanged_without_panicking() {
        let source = "if true:\nSay \"unterminated\n";
        assert_eq!(format(source), source);
    }

    #[test]
    fn normalizes_eof_without_adding_duplicate_newlines() {
        let formatted = format("if true:\n  Say \"yes\"\nend");

        assert!(formatted.ends_with('\n'));
        assert_eq!(format(&formatted), formatted);
    }

    #[test]
    fn canonicalizes_message_dispatch_spacing_without_changing_strings() {
        for source in [
            "person::greet\n",
            "person ::greet\n",
            "person:: greet\n",
            "person :: greet\n",
        ] {
            assert_eq!(format(source), "person :: greet\n");
        }
        assert_eq!(
            format("person::rename(\"a::b\") # note\n"),
            "person :: rename(\"a::b\") # note\n"
        );
    }

    #[test]
    fn canonicalizes_import_spacing_and_preserves_trailing_comments() {
        assert_eq!(
            format(
                "open   \"nested/math.si\"   as   math # library\n\
                 open \"values.si\" as values\n"
            ),
            "open \"nested/math.si\" as math # library\nopen \"values.si\" as values\n"
        );
    }

    #[test]
    fn preserves_enum_variant_separator_and_canonicalizes_match_blocks() {
        let source = "enum Result:\nOk as Int\nError as String\nend\n\
                     result is Result :: Ok(42)\n\
                     match result:\n\
                     Result :: Ok(value):\n\
                     value\n\
                     Result::Error(message):\n\
                     message\n\
                     end\n";
        let formatted = format(source);
        assert!(formatted.contains("result is Result::Ok(42)\n"));
        assert!(formatted.contains("Result::Ok(value):\n"));
        assert!(formatted.contains("Result::Error(message):\n"));
        assert_eq!(format(&formatted), formatted);
    }

    #[test]
    fn preserves_nested_tuple_and_enum_match_pattern_layout() {
        let source = "enum Result:\n    Ok as (Int, Int)\nend\n\
                     match value:\n\
                     (a,(b,c)):\n\
                     a+b+c\n\
                     Result::Ok((left,right)):\n\
                     left+right\n\
                     _:\n\
                     0\n\
                     end\n";
        let formatted = format(source);
        assert_eq!(
            formatted,
            "enum Result:\n    Ok as (Int, Int)\nend\nmatch value:\n    (a,(b,c)):\n        a + b + c\n    Result::Ok((left,right)):\n        left + right\n    _:\n        0\nend\n"
        );
        assert_eq!(format(&formatted), formatted);
    }

    #[test]
    fn preserves_guard_spacing_in_match_arm_headers() {
        let source = "enum Result:\n    Ok as Int\nend\n\
                     match result:\n\
                     Result::Ok(value) if value>10:\n\
                     value\n\
                     _:\n\
                     0\n\
                     end\n";
        let formatted = format(source);
        assert_eq!(
            formatted,
            "enum Result:\n    Ok as Int\nend\nmatch result:\n    Result::Ok(value) if value > 10:\n        value\n    _:\n        0\nend\n"
        );
        assert_eq!(format(&formatted), formatted);
    }

    #[test]
    fn canonicalizes_spacing_around_or_pattern_alternatives() {
        let source = "enum Result:\n    Ok as Int\n    Error as String\nend\n\
                     match result:\n\
                     Result::Ok(value)|Result::Error(_):\n\
                     value\n\
                     end\n";
        let formatted = format(source);
        assert!(formatted.contains("Result::Ok(value) | Result::Error(_):\n"));
        assert_eq!(format(&formatted), formatted);
    }

    #[test]
    fn formats_literal_match_arms_without_changing_literal_spelling() {
        let source = "match value:\n0:\n\"zero\"\n1|2|3:\n\"small\"\n\"hello\":\n\"greeting\"\ntrue:\n\"yes\"\nend\n";
        let formatted = format(source);
        assert_eq!(
            formatted,
            "match value:\n    0:\n        \"zero\"\n    1 | 2 | 3:\n        \"small\"\n    \"hello\":\n        \"greeting\"\n    true:\n        \"yes\"\nend\n"
        );
        assert_eq!(format(&formatted), formatted);
    }

    #[test]
    fn formats_range_pattern_bounds_without_surrounding_spaces() {
        let source = "match value:\n0 .. 10:\n1\n10 ..:\n2\n.. 10:\n3\n-10 .. 10:\n4\nend\n";
        let formatted = format(source);
        assert_eq!(
            formatted,
            "match value:\n    0..10:\n        1\n    10..:\n        2\n    ..10:\n        3\n    -10..10:\n        4\nend\n"
        );
        assert_eq!(format(&formatted), formatted);
    }

    #[test]
    fn preserves_canonical_sequence_pattern_spacing_and_formats_nested_ranges() {
        let source = "match values:\n[]:\n\"empty\"\n[1, 2, 3]:\n\"exact\"\n[[0 .. 10, 20 .. 30], [1, 2] | [3, 4]]:\n\"nested\"\nend\n";
        let formatted = format(source);
        assert_eq!(
            formatted,
            "match values:\n    []:\n        \"empty\"\n    [1, 2, 3]:\n        \"exact\"\n    [[0..10, 20..30], [1, 2] | [3, 4]]:\n        \"nested\"\nend\n"
        );
        assert_eq!(format(&formatted), formatted);
    }

    #[test]
    fn formats_rest_patterns_canonically_and_keeps_formatted_source_parseable() {
        let source = "match values:\n[...rest]:\nrest\n[head,...tail]:\nhead\n[a, b, ...rest]:\nrest\n[0 .. 10,...rest]:\nrest\n[Person(name, age),...people]:\nname\n[0,...rest]|[1,...rest]:\nrest\nend\n";
        let formatted = format(source);
        assert_eq!(
            formatted,
            "match values:\n    [...rest]:\n        rest\n    [head, ...tail]:\n        head\n    [a, b, ...rest]:\n        rest\n    [0..10, ...rest]:\n        rest\n    [Person(name, age), ...people]:\n        name\n    [0, ...rest] | [1, ...rest]:\n        rest\nend\n"
        );
        assert_eq!(format(&formatted), formatted);
        let parsed =
            crate::parser::Parser::new(crate::lexer::Lexer::new(source).tokenize().unwrap())
                .parse()
                .expect("rest patterns should parse");
        let formatted_parsed =
            crate::parser::Parser::new(crate::lexer::Lexer::new(&formatted).tokenize().unwrap())
                .parse()
                .expect("formatted rest patterns should parse");
        assert_eq!(parsed, formatted_parsed);
    }

    #[test]
    fn formats_hash_patterns_and_preserves_their_parsed_structure() {
        let source = "match record:\n{\"name\": person, extra: [0 .. 10, _]}|{name: person, extra: [11 .., _]}:\nperson\n{}:\n\"other\"\nend\n";
        let formatted = format(source);
        assert_eq!(
            formatted,
            "match record:\n    {name: person, extra: [0..10, _]} | {name: person, extra: [11.., _]}:\n        person\n    {}:\n        \"other\"\nend\n"
        );
        assert_eq!(format(&formatted), formatted);
        let parse = |source: &str| {
            crate::parser::Parser::new(crate::lexer::Lexer::new(source).tokenize().unwrap())
                .parse()
                .unwrap()
        };
        assert_eq!(parse(source), parse(&formatted));

        for pattern in [
            "{}",
            "{\"name\": name}",
            "{name: name}",
            "{\"name\": name, \"age\": 18..}",
            "{user: {name: name}}",
            "{\"items\": [head, ...tail]}",
            "{\"point\": (x, 0..)}",
        ] {
            let source = format!("match record:\n{pattern}:\n0\nend\n");
            let formatted = format(&source);
            assert_eq!(format(&formatted), formatted, "{pattern}");
            assert_eq!(parse(&source), parse(&formatted), "{pattern}");
        }
    }

    #[test]
    fn formats_alias_patterns_and_preserves_their_parsed_structure() {
        let source = "enum Result:\n    Ok as Int\nend\nmatch value:\nx@18 ..:\nx\nitem@Result::Ok(value):\nitem\nwhole@{name: name}:\nwhole\nrow@[head,...tail]:\nrow\nend\n";
        let formatted = format(source);
        assert_eq!(
            formatted,
            "enum Result:\n    Ok as Int\nend\nmatch value:\n    x @ 18..:\n        x\n    item @ Result::Ok(value):\n        item\n    whole @ {name: name}:\n        whole\n    row @ [head, ...tail]:\n        row\nend\n"
        );
        assert_eq!(format(&formatted), formatted);
        let parse = |source: &str| {
            crate::parser::Parser::new(crate::lexer::Lexer::new(source).tokenize().unwrap())
                .parse()
                .unwrap()
        };
        assert_eq!(parse(source), parse(&formatted));
    }

    #[test]
    fn formats_destructuring_targets_and_round_trips_their_ast() {
        let source = "pair is (1, 2)\n\
                      (a, b) is pair\n\
                      (a, (b, c)) is nested\n\
                      (_, value) is pair\n\
                      (a, b) -> pair\n\
                      (a, (b, c)) -> nested\n\
                      (_, b) -> pair\n\
                      [a, b] is values\n\
                      [a, b] -> values\n\
                      [head,...tail] is values\n\
                      [head,...tail] -> values\n\
                      [...rest] is values\n\
                      [...rest] -> values\n\
                      [[a, b], [c, d]] is grid\n\
                      [[head,...tail],...rows] is grid\n";
        let formatted = format(source);
        assert!(formatted.contains("[head, ...tail] is values\n"));
        assert!(formatted.contains("[[head, ...tail], ...rows] is grid\n"));
        assert_eq!(format(&formatted), formatted);
        let parse = |source: &str| {
            crate::parser::Parser::new(crate::lexer::Lexer::new(source).tokenize().unwrap())
                .parse()
                .unwrap()
        };
        assert_eq!(parse(source), parse(&formatted));
    }

    #[test]
    fn formats_all_examples_idempotently() {
        let examples = [
            "examples/01-basics/values.si",
            "examples/02-variables/assignment.si",
            "examples/03-operators/arithmetic.si",
            "examples/03-operators/logic.si",
            "examples/04-control-flow/break-continue.si",
            "examples/04-control-flow/conditionals.si",
            "examples/04-control-flow/loops.si",
            "examples/04-control-flow/while.si",
            "examples/05-functions/functions.si",
            "examples/05-functions/closures.si",
            "examples/06-collections/arrays-lists.si",
            "examples/06-collections/hash-tree.si",
            "examples/06-collections/matrices.si",
            "examples/06-collections/tuples.si",
            "examples/07-pipelines/collections.si",
            "examples/08-standard-library/builtins.si",
            "examples/08-standard-library/collections.si",
            "examples/08-standard-library/imported-values.si",
            "examples/08-standard-library/inspection.si",
            "examples/08-standard-library/strings.si",
            "examples/09-quality/message-objects.si",
            "examples/09-quality/scope-and-short-circuit.si",
            "examples/10-flow/overview.si",
            "examples/10-flow/aggregates.si",
            "examples/10-flow/checkpoint-write.si",
            "examples/10-flow/csv-cleanup.si",
            "examples/10-flow/parallel-scalar.si",
            "examples/10-flow/partition-categories.si",
            "examples/10-flow/quality-partition.si",
            "examples/11-compiler-foundations/mini-lexer.si",
            "examples/13-objects/person.si",
            "examples/14-enums/result.si",
            "examples/15-patterns/alias-patterns.si",
            "examples/15-patterns/hash-patterns.si",
            "examples/15-patterns/literal-patterns.si",
            "examples/15-patterns/or-patterns.si",
            "examples/15-patterns/pattern-guards.si",
            "examples/15-patterns/range-patterns.si",
            "examples/15-patterns/rest-patterns.si",
            "examples/15-patterns/sequence-patterns.si",
            "examples/15-patterns/struct-patterns.si",
            "examples/16-user-input/ask.si",
            "examples/99-smoke/smoke.si",
        ];

        for path in examples {
            let source = fs::read_to_string(path).expect("failed to read example source");
            let formatted = format(&source);
            assert_eq!(
                format(&formatted),
                formatted,
                "formatter is not idempotent: {path}"
            );
        }
    }
}
