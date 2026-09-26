//! cli.rs — command-line interface
//! Parses Simply commands, runs sources and tests, provides the REPL, and renders diagnostics.
//! Key components: argument parsing, test discovery, benchmarking, and debug commands.
use std::{
    env, fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    time::Instant,
};

use crate::{
    ast::{Expr, PipelineStep, Stmt},
    error::{DiagnosticCode, SimplyError, Span},
    evaluator::Evaluator,
    formatter,
    lexer::Lexer,
    parser::Parser,
    semantic::SemanticAnalyzer,
};

enum Command {
    Run,
    Tokens,
    Ast,
    Format,
    Check,
    Bench,
    Explain,
    ExplainFlow,
    Test,
    Repl,
    Help,
    Version,
}

struct Arguments {
    command: Command,
    path: Option<PathBuf>,
}

pub fn run() -> i32 {
    let raw_args: Vec<String> = env::args().collect();
    let arguments = match parse_arguments(&raw_args) {
        Ok(arguments) => arguments,
        Err(error) => {
            render_error("simply", error, "");
            if !is_source_file_shorthand(&raw_args) && raw_args.len() != 2 {
                usage();
            }
            return 2;
        }
    };

    if matches!(arguments.command, Command::Help) {
        usage();
        return 0;
    }

    if matches!(arguments.command, Command::Version) {
        println!(
            "{}",
            stdout_paint(&format!("Simply {}", env!("CARGO_PKG_VERSION")), CYAN)
        );
        return 0;
    }

    if matches!(arguments.command, Command::Repl) {
        return repl();
    }

    if matches!(arguments.command, Command::Test) {
        return run_tests();
    }

    let Some(path) = arguments.path else {
        let error = cli_error("this command requires a source file");
        render_error("simply", error, "");
        usage();
        return 2;
    };
    if path.extension().and_then(|extension| extension.to_str()) != Some("si") {
        render_cli_error(&path, "Simply source files must use the .si extension");
        return 2;
    }

    let source = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) => {
            let diagnostic = SimplyError::Runtime {
                span: Span::new(0, 0),
                code: DiagnosticCode::RuntimeImport,
                message: format!("could not read `{}`: {error}", path.display()),
            };
            render_error(&path, diagnostic, "");
            return 1;
        }
    };

    let result = match arguments.command {
        Command::Tokens => debug_tokens(&source),
        Command::Ast => debug_ast(&source),
        Command::Format => {
            validate_source(&source).map(|_| print!("{}", formatter::format(&source)))
        }
        Command::Check => {
            println!(
                "{}",
                stdout_paint(&format!("Checking {}...", path.display()), CYAN)
            );
            check_source(&source).map(|_| println!("{}", stdout_paint("No errors found.", GREEN)))
        }
        Command::Bench => benchmark_source(&source, &path),
        Command::Explain => explain_source(&source, &path),
        Command::ExplainFlow => explain_flow_source(&source, &path),
        Command::Run => Evaluator::new().run_file(&path),
        Command::Test | Command::Repl | Command::Help | Command::Version => Ok(()),
    };

    match result {
        Ok(()) => 0,
        Err(error) => {
            render_error(&path, error, &source);
            if matches!(arguments.command, Command::Check) {
                eprintln!("\nFound 1 error.");
            }
            1
        }
    }
}

fn is_source_file_shorthand(raw_args: &[String]) -> bool {
    raw_args.len() == 2
        && Path::new(&raw_args[1])
            .extension()
            .and_then(|extension| extension.to_str())
            == Some("si")
}

fn parse_arguments(raw_args: &[String]) -> Result<Arguments, SimplyError> {
    if raw_args.len() == 1 {
        return Ok(Arguments {
            command: Command::Help,
            path: None,
        });
    }

    if raw_args.len() == 2 && (raw_args[1] == "--help" || raw_args[1] == "-h") {
        return Ok(Arguments {
            command: Command::Help,
            path: None,
        });
    }

    if raw_args.len() == 2 && (raw_args[1] == "--version" || raw_args[1] == "-V") {
        return Ok(Arguments {
            command: Command::Version,
            path: None,
        });
    }

    if raw_args.len() == 2 && raw_args[1] == "repl" {
        return Ok(Arguments {
            command: Command::Repl,
            path: None,
        });
    }

    if raw_args.len() == 2 && raw_args[1] == "test" {
        return Ok(Arguments {
            command: Command::Test,
            path: None,
        });
    }

    if is_source_file_shorthand(raw_args) {
        return Err(cli_error(format!(
            "source file `{}` was provided without a command; use `simply run {}` (or `simply check {}` to validate it)",
            raw_args[1], raw_args[1], raw_args[1]
        )));
    }

    if raw_args.len() == 2 {
        return Err(cli_error(format!(
            "unknown command or option `{}`; use `simply --help` to see available commands",
            raw_args[1]
        )));
    }

    if raw_args.len() != 3 {
        return Err(cli_error("expected a command and source file"));
    }

    let command = match raw_args[1].as_str() {
        "run" => Command::Run,
        "tokens" => Command::Tokens,
        "ast" => Command::Ast,
        "fmt" => Command::Format,
        "check" => Command::Check,
        "bench" => Command::Bench,
        "explain" => Command::Explain,
        "explain-flow" => Command::ExplainFlow,
        other => return Err(cli_error(format!("unknown option `{other}`"))),
    };

    Ok(Arguments {
        command,
        path: Some(PathBuf::from(&raw_args[2])),
    })
}

fn usage() {
    println!(
        "{}",
        stdout_paint(&format!("Simply {}", env!("CARGO_PKG_VERSION")), CYAN)
    );
    println!();
    println!("A small, readable language for scripts and experiments.");
    println!();
    println!("{}", stdout_paint("USAGE:", YELLOW));
    println!("    simply <COMMAND> [OPTIONS]");
    println!();
    println!("{}", stdout_paint("COMMANDS:", YELLOW));
    println!("    run <file.si>      Execute a Simply program");
    println!("    check <file.si>    Validate syntax and semantics");
    println!("    fmt <file.si>      Format a Simply program to stdout");
    println!("    tokens <file.si>   Print the lexer token stream");
    println!("    ast <file.si>      Print the parsed abstract syntax tree");
    println!("    bench <file.si>    Benchmark the compiler and runtime");
    println!("    explain <file.si>  Explain program structure and runtime model");
    println!("    explain-flow <file.si>  Print a Flow execution plan");
    println!("    repl               Start the interactive evaluator");
    println!();
    println!("{}", stdout_paint("OPTIONS:", YELLOW));
    println!("    -h, --help         Print this help message");
    println!("    -V, --version      Print version information");
    println!();
    println!("{}", stdout_paint("EXAMPLES:", YELLOW));
    println!("    simply run examples/99-smoke/smoke.si");
    println!("    simply fmt examples/01-basics/values.si");
    println!("    simply check examples/99-smoke/smoke.si");
}

#[derive(Default)]
struct ExplainStats {
    statements: usize,
    bindings: usize,
    mutable_bindings: usize,
    functions: usize,
    nested_functions: usize,
    calls: usize,
    loops: usize,
    try_blocks: usize,
    catch_clauses: usize,
    finally_blocks: usize,
    throws: usize,
}

fn explain_source(source: &str, path: &Path) -> Result<(), SimplyError> {
    let tokens = Lexer::new(source).tokenize()?;
    let program = Parser::new(tokens).parse()?;
    SemanticAnalyzer::new().analyze(&program)?;

    let mut stats = ExplainStats::default();
    collect_statement_stats(&program.statements, &mut stats, false);
    println!("{}", stdout_paint("Simply explanation", CYAN));
    println!("file: {}", path.display());
    println!("status: valid");
    println!();
    println!("structure:");
    println!("  statements:       {}", stats.statements);
    println!("  bindings:         {}", stats.bindings);
    println!("  mutable bindings: {}", stats.mutable_bindings);
    println!("  functions:        {}", stats.functions);
    println!("  nested functions: {}", stats.nested_functions);
    println!("  function calls:   {}", stats.calls);
    println!("  loops:            {}", stats.loops);
    println!("  try blocks:       {}", stats.try_blocks);
    println!("  catch clauses:    {}", stats.catch_clauses);
    println!("  finally blocks:   {}", stats.finally_blocks);
    println!("  throws:           {}", stats.throws);
    println!();
    println!("runtime model:");
    println!("  bindings: immutable by default");
    println!("  collections: Rc copy-on-write");
    println!("  closures: lexical snapshot capture");
    Ok(())
}

fn explain_flow_source(source: &str, path: &Path) -> Result<(), SimplyError> {
    let tokens = Lexer::new(source).tokenize()?;
    let program = Parser::new(tokens).parse()?;
    SemanticAnalyzer::new().analyze(&program)?;

    let flows: Vec<_> = program
        .statements
        .iter()
        .filter_map(|statement| match statement {
            Stmt::Located { statement, .. } => match statement.as_ref() {
                Stmt::Flow {
                    name,
                    source,
                    steps,
                } => Some((name, source, steps)),
                _ => None,
            },
            Stmt::Flow {
                name,
                source,
                steps,
            } => Some((name, source, steps)),
            _ => None,
        })
        .collect();

    println!("{}", stdout_paint("Simply Flow execution plan", CYAN));
    println!("file: {}", path.display());
    println!("status: valid");
    if flows.is_empty() {
        println!("program kind: non-flow");
        println!("flows: none");
        println!("note: no declarative `flow ... end` declarations were found");
        return Ok(());
    }
    println!("program kind: flow");
    println!("flows: {}", flows.len());
    for (index, (name, source, steps)) in flows.iter().enumerate() {
        if index > 0 {
            println!();
        }
        let source_kind = flow_source_kind(source);
        let streaming = matches!(source_kind, "CSV stream" | "lazy range");
        let terminal = steps
            .iter()
            .rev()
            .find_map(flow_terminal_name)
            .unwrap_or("none");
        let mut fusible_steps = steps
            .iter()
            .take_while(|step| flow_terminal_name(step).is_none());
        let fusion = fusible_steps
            .clone()
            .any(|step| matches!(step, PipelineStep::Where(_) | PipelineStep::Derive(_)))
            && fusible_steps
                .all(|step| matches!(step, PipelineStep::Where(_) | PipelineStep::Derive(_)));
        println!("flow {}:", name);
        println!("  source kind: {}", source_kind);
        println!(
            "  operators: {}",
            steps
                .iter()
                .map(flow_step_name)
                .collect::<Vec<_>>()
                .join(" -> ")
        );
        println!("  terminal: {}", terminal);
        println!(
            "  mode: {}",
            if streaming {
                "streaming"
            } else {
                "materialized"
            }
        );
        println!(
            "  fusion: {}",
            if fusion {
                "available (where/derive chain)"
            } else {
                "unavailable (terminal or control step)"
            }
        );
    }
    Ok(())
}

fn flow_source_kind(source: &Expr) -> &'static str {
    match source {
        Expr::Call { name, .. } if name == "csv_rows" => "CSV stream",
        Expr::Call { name, .. } if name == "range" => "lazy range",
        Expr::Array(_) => "array",
        Expr::List(_) => "list",
        Expr::Tuple(_) => "tuple",
        Expr::Identifier(_) => "bound collection",
        _ => "expression",
    }
}

fn flow_step_name(step: &PipelineStep) -> &'static str {
    match step {
        PipelineStep::Where(_) => "where",
        PipelineStep::Derive(_) => "derive",
        PipelineStep::Partition { .. } => "partition",
        PipelineStep::Sum => "sum",
        PipelineStep::Count => "count",
        PipelineStep::Average => "average",
        PipelineStep::Min => "min",
        PipelineStep::Max => "max",
        PipelineStep::WriteCsv(_) => "write_csv",
        PipelineStep::Chunk(_) => "chunk",
        PipelineStep::Parallel(_) => "parallel (validated scalar workers)",
        PipelineStep::Checkpoint(_) => "checkpoint",
    }
}

fn flow_terminal_name(step: &PipelineStep) -> Option<&'static str> {
    match step {
        PipelineStep::Sum => Some("sum"),
        PipelineStep::Count => Some("count"),
        PipelineStep::Average => Some("average"),
        PipelineStep::Min => Some("min"),
        PipelineStep::Max => Some("max"),
        PipelineStep::Partition { .. } => Some("partition"),
        PipelineStep::WriteCsv(_) => Some("write_csv"),
        _ => None,
    }
}

fn collect_statement_stats(statements: &[Stmt], stats: &mut ExplainStats, inside_function: bool) {
    for statement in statements {
        let statement = match statement {
            Stmt::Located { statement, .. } => statement.as_ref(),
            statement => statement,
        };
        stats.statements += 1;
        match statement {
            Stmt::Located { statement, .. } => collect_statement_stats(
                std::slice::from_ref(statement.as_ref()),
                stats,
                inside_function,
            ),
            Stmt::Assign { mutable, value, .. } => {
                stats.bindings += 1;
                stats.mutable_bindings += usize::from(*mutable);
                collect_expression_stats(value, stats);
            }
            Stmt::Flow { source, steps, .. } => {
                stats.bindings += 1;
                collect_expression_stats(source, stats);
                for step in steps {
                    match step {
                        PipelineStep::Where(expression)
                        | PipelineStep::Derive(expression)
                        | PipelineStep::WriteCsv(expression)
                        | PipelineStep::Checkpoint(expression) => {
                            collect_expression_stats(expression, stats)
                        }
                        _ => {}
                    }
                }
            }
            Stmt::Destructure { names, value } => {
                stats.bindings += names.len();
                stats.mutable_bindings += names.iter().filter(|(_, mutable)| *mutable).count();
                collect_expression_stats(value, stats);
            }
            Stmt::Function { body, .. } => {
                stats.functions += 1;
                stats.nested_functions += usize::from(inside_function);
                collect_statement_stats(body, stats, true);
            }
            Stmt::For { iterable, body, .. } => {
                stats.loops += 1;
                collect_expression_stats(iterable, stats);
                collect_statement_stats(body, stats, inside_function);
            }
            Stmt::While { condition, body } => {
                stats.loops += 1;
                collect_expression_stats(condition, stats);
                collect_statement_stats(body, stats, inside_function);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                collect_expression_stats(condition, stats);
                collect_statement_stats(then_branch, stats, inside_function);
                collect_statement_stats(else_branch, stats, inside_function);
            }
            Stmt::Try {
                try_body,
                catches,
                finally_body,
            } => {
                stats.try_blocks += 1;
                stats.catch_clauses += catches.len();
                stats.finally_blocks += usize::from(!finally_body.is_empty());
                collect_statement_stats(try_body, stats, inside_function);
                for catch in catches {
                    collect_statement_stats(&catch.body, stats, inside_function);
                }
                collect_statement_stats(finally_body, stats, inside_function);
            }
            Stmt::Throw(expression) => {
                stats.throws += 1;
                collect_expression_stats(expression, stats);
            }
            Stmt::Say(expression) | Stmt::Expression(expression) | Stmt::Return(expression) => {
                collect_expression_stats(expression, stats)
            }
            Stmt::Reassign { value, .. } | Stmt::CollectionOp { value, .. } => {
                collect_expression_stats(value, stats)
            }
            Stmt::SetIndex { index, value, .. } => {
                collect_expression_stats(index, stats);
                collect_expression_stats(value, stats);
            }
            Stmt::Import { .. } | Stmt::Break | Stmt::Continue => {}
        }
    }
}

fn collect_expression_stats(expression: &Expr, stats: &mut ExplainStats) {
    match expression {
        Expr::Call { arguments, .. } => {
            stats.calls += 1;
            for argument in arguments {
                collect_expression_stats(argument, stats);
            }
        }
        Expr::Unary { operand, .. } => collect_expression_stats(operand, stats),
        Expr::Binary { left, right, .. } => {
            collect_expression_stats(left, stats);
            collect_expression_stats(right, stats);
        }
        Expr::Array(values) | Expr::List(values) | Expr::Tuple(values) | Expr::Matrix(values) => {
            for value in values {
                collect_expression_stats(value, stats);
            }
        }
        Expr::Index { target, index } => {
            collect_expression_stats(target, stats);
            collect_expression_stats(index, stats);
        }
        Expr::Field { target, .. } => collect_expression_stats(target, stats),
        Expr::Hash(entries) | Expr::Tree(entries) => {
            for (_, value) in entries {
                collect_expression_stats(value, stats);
            }
        }
        Expr::Pipeline { source, steps } => {
            collect_expression_stats(source, stats);
            for step in steps {
                if let crate::ast::PipelineStep::Where(value)
                | crate::ast::PipelineStep::Derive(value) = step
                {
                    collect_expression_stats(value, stats);
                }
            }
        }
        Expr::Literal(_) | Expr::Identifier(_) => {}
    }
}

fn benchmark_source(source: &str, path: &Path) -> Result<(), SimplyError> {
    const ITERATIONS: u32 = 10;
    let mut lex_nanos = 0u128;
    let mut parse_nanos = 0u128;
    let mut semantic_nanos = 0u128;
    let mut runtime_nanos = 0u128;
    let mut parsed_program = None;

    for _ in 0..ITERATIONS {
        let started = Instant::now();
        let tokens = Lexer::new(source).tokenize()?;
        lex_nanos += started.elapsed().as_nanos();

        let started = Instant::now();
        let program = Parser::new(tokens).parse()?;
        parse_nanos += started.elapsed().as_nanos();

        let started = Instant::now();
        SemanticAnalyzer::new().analyze(&program)?;
        semantic_nanos += started.elapsed().as_nanos();
        parsed_program = Some(program);
    }

    let program =
        parsed_program.ok_or_else(|| cli_error("benchmark did not produce a parsed program"))?;
    let resolved_path = fs::canonicalize(path).map_err(|error| SimplyError::Runtime {
        span: Span::new(0, 0),
        code: DiagnosticCode::RuntimeImport,
        message: format!("could not open `{}`: {error}", path.display()),
    })?;
    for _ in 0..ITERATIONS {
        let started = Instant::now();
        Evaluator::new().run_silent_resolved(&program, &resolved_path)?;
        runtime_nanos += started.elapsed().as_nanos();
    }

    let average = |nanos: u128| nanos as f64 / ITERATIONS as f64 / 1_000_000.0;
    println!(
        "{}",
        stdout_paint(&format!("benchmark ({ITERATIONS} iterations)"), CYAN)
    );
    println!("lex:      {:.3} ms", average(lex_nanos));
    println!("parse:    {:.3} ms", average(parse_nanos));
    println!("semantic: {:.3} ms", average(semantic_nanos));
    println!("runtime:  {:.3} ms", average(runtime_nanos));
    println!(
        "total:    {:.3} ms",
        average(lex_nanos + parse_nanos + semantic_nanos + runtime_nanos)
    );
    Ok(())
}

fn run_tests() -> i32 {
    println!("{}", stdout_paint("running SimplyLang tests...", CYAN));
    let tests = match discover_tests(Path::new("tests")) {
        Ok(tests) => tests,
        Err(error) => {
            render_error(PathBuf::from("tests"), error, "");
            return 1;
        }
    };

    let mut passed = 0;
    let mut failed = 0;
    for path in tests {
        let result = Evaluator::new().run_file(&path);
        match result {
            Ok(()) => {
                passed += 1;
                println!("{} {}", stdout_paint("PASS", GREEN), path.display());
            }
            Err(error) => {
                failed += 1;
                println!("{} {}", stdout_paint("FAIL", RED), path.display());
                let source = fs::read_to_string(&path).unwrap_or_default();
                render_error(&path, error, &source);
            }
        }
    }

    if failed == 0 {
        println!("\n{}", stdout_paint("test result: OK", GREEN));
    } else {
        println!("\n{}", stdout_paint("test result: FAILED", RED));
    }
    println!("{passed} passed; {failed} failed");
    i32::from(failed != 0)
}

fn discover_tests(directory: &Path) -> Result<Vec<PathBuf>, SimplyError> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(SimplyError::Runtime {
                span: Span::new(0, 0),
                code: DiagnosticCode::RuntimeGeneral,
                message: format!(
                    "could not read test directory `{}`: {error}",
                    directory.display()
                ),
            });
        }
    };

    let mut tests = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| SimplyError::Runtime {
            span: Span::new(0, 0),
            code: DiagnosticCode::RuntimeGeneral,
            message: format!("could not read test directory entry: {error}"),
        })?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|value| value.to_str()) == Some("si") {
            tests.push(path);
        }
    }
    tests.sort();
    Ok(tests)
}

fn repl() -> i32 {
    println!(
        "{}",
        stdout_paint(&format!("Simply {}", env!("CARGO_PKG_VERSION")), CYAN)
    );
    let mut evaluator = Evaluator::new();
    let stdin = io::stdin();
    let mut input = String::new();

    loop {
        print!("{}", stdout_paint("> ", CYAN));
        if io::stdout().flush().is_err() {
            return 1;
        }
        input.clear();
        match stdin.read_line(&mut input) {
            Ok(0) => return 0,
            Ok(_) => {
                let tokens = match Lexer::new(&input).tokenize() {
                    Ok(tokens) => tokens,
                    Err(error) => {
                        render_error("<repl>", error, &input);
                        continue;
                    }
                };
                let program = match Parser::new(tokens).parse() {
                    Ok(program) => program,
                    Err(error) => {
                        render_error("<repl>", error, &input);
                        continue;
                    }
                };
                if let Err(error) = evaluator.run_repl(&program) {
                    render_error("<repl>", error, &input);
                }
            }
            Err(error) => {
                render_error(
                    "<repl>",
                    SimplyError::Runtime {
                        span: Span::new(0, 0),
                        code: DiagnosticCode::RuntimeGeneral,
                        message: format!("could not read REPL input: {error}"),
                    },
                    "",
                );
                return 1;
            }
        }
    }
}

fn debug_tokens(source: &str) -> Result<(), SimplyError> {
    let tokens = Lexer::new(source).tokenize()?;
    for token in tokens {
        println!("{token:?}");
    }
    Ok(())
}

fn debug_ast(source: &str) -> Result<(), SimplyError> {
    let tokens = Lexer::new(source).tokenize()?;
    let program = Parser::new(tokens).parse()?;
    println!("{program:#?}");
    Ok(())
}

fn render_cli_error(path: &PathBuf, message: impl Into<String>) {
    render_error(
        path,
        SimplyError::Command {
            span: Span::new(0, 0),
            code: DiagnosticCode::CliUsage,
            message: message.into(),
        },
        "",
    );
}

fn check_source(source: &str) -> Result<(), SimplyError> {
    validate_source(source)
}

fn validate_source(source: &str) -> Result<(), SimplyError> {
    let tokens = Lexer::new(source).tokenize()?;
    let program = Parser::new(tokens).parse()?;
    SemanticAnalyzer::new().analyze(&program)
}

fn cli_error(message: impl Into<String>) -> SimplyError {
    SimplyError::Command {
        span: Span::new(0, 0),
        code: DiagnosticCode::CliUsage,
        message: message.into(),
    }
}

fn render_error(path: impl AsRef<std::path::Path>, error: SimplyError, source: &str) {
    let terminal_width =
        terminal_size::terminal_size().map(|(terminal_size::Width(width), _)| width as usize);
    let rendered = error.render_with_terminal_width(
        &path.as_ref().display().to_string(),
        source,
        terminal_width,
    );
    eprintln!("{}", stderr_paint(&rendered, RED));
}

struct AnsiColor(&'static str);

const CYAN: AnsiColor = AnsiColor("\x1b[36;1m");
const GREEN: AnsiColor = AnsiColor("\x1b[32;1m");
const RED: AnsiColor = AnsiColor("\x1b[31;1m");
const YELLOW: AnsiColor = AnsiColor("\x1b[33;1m");

fn stdout_paint(text: &str, color: AnsiColor) -> String {
    paint(text, color, io::stdout().is_terminal())
}

fn stderr_paint(text: &str, color: AnsiColor) -> String {
    paint(text, color, io::stderr().is_terminal())
}

fn paint(text: &str, color: AnsiColor, enabled: bool) -> String {
    if enabled {
        format!("{}{text}\x1b[0m", color.0)
    } else {
        text.to_owned()
    }
}
