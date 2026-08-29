use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "apisnap",
    version,
    about = "Language-agnostic API snapshot regression testing CLI with deterministic auto-masking"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Path to configuration file (TOML or YAML).
    #[arg(long, global = true, default_value = "apisnap.toml")]
    pub config: String,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Scaffold a new starter apisnap configuration file.
    Init {
        /// Destination filename for the starter configuration.
        #[arg(long, default_value = "apisnap.toml")]
        output: String,
    },
    /// Execute configured endpoints, auto-mask volatile data, and save fresh snapshots.
    Record {
        /// Only record a specific endpoint by name.
        #[arg(long)]
        endpoint: Option<String>,
        /// Override the concurrency level for parallel requests.
        #[arg(long)]
        concurrency: Option<usize>,
    },
    /// Execute endpoints and compare live responses against saved snapshots.
    Test {
        /// Only test a specific endpoint by name.
        #[arg(long)]
        endpoint: Option<String>,
        /// Override the concurrency level for parallel requests.
        #[arg(long)]
        concurrency: Option<usize>,
        /// Emit machine-readable JSON summary for CI environments.
        #[arg(long)]
        ci: bool,
    },
    /// Interactively walk through snapshot mismatches and accept/reject changes.
    Review {
        /// Only review a specific endpoint by name.
        #[arg(long)]
        endpoint: Option<String>,
    },
}
