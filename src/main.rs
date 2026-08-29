use apisnap::cli::{handle_init, handle_record, handle_review, handle_test, Cli, Command};
use clap::Parser;
use colored::Colorize;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Command::Init { output } => handle_init(&output),
        Command::Record {
            endpoint,
            concurrency,
        } => handle_record(&cli.config, endpoint.as_deref(), concurrency).await,
        Command::Test {
            endpoint,
            concurrency,
            ci,
        } => handle_test(&cli.config, endpoint.as_deref(), concurrency, ci).await,
        Command::Review { endpoint } => {
            handle_review(&cli.config, endpoint.as_deref()).await
        }
    };

    match result {
        Ok(_) => ExitCode::SUCCESS,
        Err(err) => {
            let code = err.exit_code();
            match code {
                1 => {
                    // Diff mismatch is already formatted by reporter
                }
                2 => {
                    eprintln!("{} {}", "[NETWORK ERROR]".red().bold(), err);
                }
                3 => {
                    eprintln!("{} {}", "[CONFIG/IO ERROR]".red().bold(), err);
                }
                _ => {
                    eprintln!("{} {}", "[ERROR]".red().bold(), err);
                }
            }
            ExitCode::from(code as u8)
        }
    }
}
