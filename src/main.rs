mod ast;
mod error;
mod evaluator;
mod formatter;
mod lexer;
mod parser;

use std::{env, fs, path::Path, process};

use error::SimplyError;
use evaluator::Evaluator;
use lexer::Lexer;
use parser::Parser;

fn usage() {
    println!("Simply 0.2.0");
    println!("Usage:");
    println!("  simply <file.si>");
    println!("  simply --tokens <file.si>");
    println!("  simply --ast <file.si>");
    println!("  simply --format <file.si>");
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

fn format_source(source: &str) -> String {
    formatter::format(source)
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 && args.len() != 3 {
        usage();
        process::exit(2);
    }

    if args.len() == 2 && (args[1] == "--help" || args[1] == "-h") {
        usage();
        return;
    }

    let (mode, path) = if args.len() == 3 {
        match args[1].as_str() {
            "--tokens" => ("tokens", &args[2]),
            "--ast" => ("ast", &args[2]),
            "--format" => ("format", &args[2]),
            other => {
                eprintln!("error: unknown option `{other}`");
                usage();
                process::exit(2);
            }
        }
    } else {
        ("run", &args[1])
    };

    if !path.ends_with(".si") {
        eprintln!("error: Simply source files must use the .si extension");
        process::exit(2);
    }

    let source = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) => {
            eprintln!("error: could not read `{path}`: {err}");
            process::exit(1);
        }
    };

    let result = match mode {
        "tokens" => debug_tokens(&source),
        "ast" => debug_ast(&source),
        "format" => {
            print!("{}", format_source(&source));
            Ok(())
        }
        _ => Evaluator::new().run_file(Path::new(path)),
    };

    if let Err(err) = result {
        eprintln!("{err}");
        process::exit(1);
    }
}
