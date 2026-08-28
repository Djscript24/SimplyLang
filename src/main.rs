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
