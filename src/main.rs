use clap::Parser;
use pbuildrs::cli;
use std::process;

fn main() {
    let args = cli::Args::parse();

    if let Err(e) = pbuildrs::cli::run(args) {
        eprintln!("{e}");

        process::exit(1);
    }
}
