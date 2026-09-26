//! main.rs — executable entry point
//! Declares the Simply compiler modules and starts command-line execution.
//! Key component: main delegates to the CLI runner.
mod ast;
mod cli;
mod error;
mod evaluator;
mod formatter;
mod lexer;
mod parser;
mod runtime;
mod semantic;
mod types;

fn main() {
    std::process::exit(cli::run());
}
