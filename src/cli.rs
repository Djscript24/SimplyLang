use std::{env, fs, path::PathBuf};

use crate::{
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
    Help,
}

struct Arguments {
    command: Command,
    path: PathBuf,
}

pub fn run() -> i32 {
    let raw_args: Vec<String> = env::args().collect();
    let arguments = match parse_arguments(&raw_args) {
        Ok(arguments) => arguments,
        Err(error) => {
            render_error("simply", error, "");
            usage();
            return 2;
        }
    };

    if matches!(arguments.command, Command::Help) {
        usage();
        return 0;
    }

    if arguments
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        != Some("si")
    {
        render_cli_error(
            &arguments.path,
            "Simply source files must use the .si extension",
        );
        return 2;
    }

    let source = match fs::read_to_string(&arguments.path) {
        Ok(content) => content,
        Err(error) => {
            let diagnostic = SimplyError::Runtime {
                span: Span::new(0, 0),
                code: DiagnosticCode::RuntimeImport,
                message: format!("could not read `{}`: {error}", arguments.path.display()),
            };
            render_error(&arguments.path, diagnostic, "");
            return 1;
        }
    };

    let result = match arguments.command {
        Command::Tokens => debug_tokens(&source),
        Command::Ast => debug_ast(&source),
        Command::Format => {
            print!("{}", formatter::format(&source));
            Ok(())
        }
        Command::Check => {
            println!("Checking {}...", arguments.path.display());
            check_source(&source).map(|_| println!("No errors found."))
        }
        Command::Run => Evaluator::new().run_file(&arguments.path),
        Command::Help => Ok(()),
    };

    match result {
        Ok(()) => 0,
        Err(error) => {
            render_error(&arguments.path, error, &source);
            if matches!(arguments.command, Command::Check) {
                eprintln!("\nFound 1 error.");
            }
            1
        }
    }
}

fn parse_arguments(raw_args: &[String]) -> Result<Arguments, SimplyError> {
    if raw_args.len() != 2 && raw_args.len() != 3 {
        return Err(cli_error("expected a command and source file"));
    }

    if raw_args.len() == 2 && (raw_args[1] == "--help" || raw_args[1] == "-h") {
        return Ok(Arguments {
            command: Command::Help,
            path: PathBuf::new(),
        });
    }

    let (command, path) = if raw_args.len() == 3 {
        let command = match raw_args[1].as_str() {
            "--tokens" => Command::Tokens,
            "--ast" => Command::Ast,
            "--format" => Command::Format,
            "check" | "--check" => Command::Check,
            other => return Err(cli_error(format!("unknown option `{other}`"))),
        };
        (command, PathBuf::from(&raw_args[2]))
    } else {
        (Command::Run, PathBuf::from(&raw_args[1]))
    };

    Ok(Arguments { command, path })
}

fn usage() {
    println!("Simply 0.1.0");
    println!("Usage:");
    println!("  simply <file.si>");
    println!("  simply --tokens <file.si>");
    println!("  simply --ast <file.si>");
    println!("  simply --format <file.si>");
    println!("  simply check <file.si>");
    println!("  simply --check <file.si>");
    println!("  simply --help");
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
        SimplyError::Runtime {
            span: Span::new(0, 0),
            code: DiagnosticCode::RuntimeGeneral,
            message: message.into(),
        },
        "",
    );
}

fn check_source(source: &str) -> Result<(), SimplyError> {
    let tokens = Lexer::new(source).tokenize()?;
    let program = Parser::new(tokens).parse()?;
    SemanticAnalyzer::new().analyze(&program)
}

fn cli_error(message: impl Into<String>) -> SimplyError {
    SimplyError::Runtime {
        span: Span::new(0, 0),
        code: DiagnosticCode::RuntimeGeneral,
        message: message.into(),
    }
}

fn render_error(path: impl AsRef<std::path::Path>, error: SimplyError, source: &str) {
    eprintln!(
        "{}",
        error.render(&path.as_ref().display().to_string(), source)
    );
}
