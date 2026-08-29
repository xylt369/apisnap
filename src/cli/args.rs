use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "apisnap",
    version = env!("CARGO_PKG_VERSION"),
    about = "Language-agnostic API snapshot regression testing CLI with deterministic auto-masking",
    long_about = "ApiSnap is a high-performance Rust CLI that captures live HTTP/gRPC API responses, automatically masks volatile dynamic noise (UUIDs, timestamps, tokens), and runs sub-millisecond AST regression diffs in CI pipelines."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Scaffold a new starter apisnap.toml configuration file
    Init(InitArgs),

    /// Query endpoints and record baseline golden snapshots (.snap.json)
    Record(RecordArgs),

    /// Execute live requests against endpoints and compare AST diffs against recorded snapshots
    Test(TestArgs),

    /// Interactively review snapshot differences in the terminal and accept/reject changes
    Review(ReviewArgs),

    /// Bidirectional OpenAPI 3.1 synchronization and contract drift verification
    Openapi(OpenApiSubcommand),
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Target path for scaffolded configuration file
    #[arg(short, long, default_value = "apisnap.toml")]
    pub output: String,
}

#[derive(Debug, Args)]
pub struct RecordArgs {
    /// Path to apisnap.toml or apisnap.yaml configuration file
    #[arg(short, long, default_value = "apisnap.toml")]
    pub config: String,

    /// Optional name of a single endpoint to record
    #[arg(short, long)]
    pub endpoint: Option<String>,

    /// Concurrency override
    #[arg(short, long)]
    pub concurrency: Option<usize>,
}

#[derive(Debug, Args)]
pub struct TestArgs {
    /// Path to apisnap.toml or apisnap.yaml configuration file
    #[arg(short, long, default_value = "apisnap.toml")]
    pub config: String,

    /// Optional name of a single endpoint to test
    #[arg(short, long)]
    pub endpoint: Option<String>,

    /// Concurrency override
    #[arg(short, long)]
    pub concurrency: Option<usize>,

    /// Output machine-readable JSON diff report for CI pipelines
    #[arg(long)]
    pub ci: bool,
}

#[derive(Debug, Args)]
pub struct ReviewArgs {
    /// Path to apisnap.toml or apisnap.yaml configuration file
    #[arg(short, long, default_value = "apisnap.toml")]
    pub config: String,

    /// Optional name of a single endpoint to review
    #[arg(short, long)]
    pub endpoint: Option<String>,
}

#[derive(Debug, Args)]
pub struct OpenApiSubcommand {
    #[command(subcommand)]
    pub action: OpenApiActions,
}

#[derive(Debug, Subcommand)]
pub enum OpenApiActions {
    /// Synthesize an OpenAPI 3.1 specification YAML from recorded snapshot files
    Generate(OpenApiGenerateArgs),

    /// Verify recorded snapshots against an existing OpenAPI specification file
    Verify(OpenApiVerifyArgs),
}

#[derive(Debug, Args)]
pub struct OpenApiGenerateArgs {
    /// Path to apisnap.toml configuration file
    #[arg(short, long, default_value = "apisnap.toml")]
    pub config: String,

    /// Target output path for synthesized OpenAPI YAML
    #[arg(short, long, default_value = "openapi.yaml")]
    pub output: String,
}

#[derive(Debug, Args)]
pub struct OpenApiVerifyArgs {
    /// Path to apisnap.toml configuration file
    #[arg(short, long, default_value = "apisnap.toml")]
    pub config: String,

    /// Path to the existing OpenAPI specification file (YAML or JSON)
    #[arg(short, long, default_value = "openapi.yaml")]
    pub spec: String,
}
