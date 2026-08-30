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

    /// Ingest endpoints from external sources (cURL, Postman collection, or HAR file)
    Import(ImportArgs),

    /// Query endpoints and record baseline golden snapshots (.snap.json)
    Record(RecordArgs),

    /// Execute live requests against endpoints and compare AST diffs against recorded snapshots
    Test(TestArgs),

    /// Interactively review snapshot differences in the terminal and accept/reject changes
    Review(ReviewArgs),

    /// Whitelist an intentional schema breaking change in the approval ledger
    ApproveDiff(ApproveDiffArgs),

    /// API Behavioral Timeline (Time Machine for API state and latency evolution)
    Timeline(TimelineArgs),

    /// Cross-service blast radius analysis calculating cascading impact on downstream consumers
    BlastRadius(BlastRadiusArgs),

    /// Local transparent capture proxy that records golden snapshots from real app traffic
    Capture(CaptureArgs),

    /// Run intelligent boundary fuzzing to detect 500 crashes and unhandled stack trace leaks
    Fuzz(FuzzArgs),

    /// Bidirectional OpenAPI 3.1 synchronization and contract drift verification
    Openapi(OpenApiSubcommand),

    /// Merkle DAG Content-Addressable Storage (CAS) inspection and deduplication management
    Cas(CasArgs),

    /// Passive zero-overhead kernel traffic sniffing via Linux eBPF TC egress hooks
    Sniff(SniffArgs),

    /// Envoy / Proxy-Wasm real-time shadow traffic differ proxy
    Shadow(ShadowArgs),
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Target path for scaffolded configuration file
    #[arg(short, long, default_value = "apisnap.toml")]
    pub output: String,
}

#[derive(Debug, Args)]
pub struct ImportArgs {
    /// Path to config file to update
    #[arg(short, long, default_value = "apisnap.toml")]
    pub config: String,

    #[command(subcommand)]
    pub source: ImportSource,
}

#[derive(Debug, Subcommand)]
pub enum ImportSource {
    /// Import an endpoint from a raw cURL command
    Curl { command: String },

    /// Import endpoints from a Postman Collection JSON file (v2.0 or v2.1)
    Postman { file: String },

    /// Import API endpoints from an HTTP Archive (HAR) file exported from DevTools
    Har { file: String },
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

    /// Enable Merkle DAG Content-Addressable Storage (CAS) deduplication
    #[arg(long)]
    pub cas: bool,

    /// Number of probe iterations to run for adaptive noise learning
    #[arg(long)]
    pub learn: Option<usize>,
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

    /// Output GitHub Actions Pull Request Markdown comment format
    #[arg(long)]
    pub pr_comment: bool,

    /// Baseline branch or pointer name for team CAS comparison (e.g. main)
    #[arg(long)]
    pub baseline: Option<String>,

    /// Candidate branch or pointer name for team CAS comparison (e.g. pr-4521)
    #[arg(long)]
    pub candidate: Option<String>,
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
pub struct ApproveDiffArgs {
    /// Name of the endpoint with intentional changes
    #[arg(short, long)]
    pub endpoint: String,

    /// Author or developer whitelisting the change
    #[arg(short, long, default_value = "developer")]
    pub author: String,

    /// Rationale for the intentional breaking change
    #[arg(short, long)]
    pub reason: String,

    /// Snapshot directory containing approval ledger
    #[arg(short, long, default_value = "__snapshots__")]
    pub snapshot_dir: String,
}

#[derive(Debug, Args)]
pub struct TimelineArgs {
    /// Snapshot or CAS base directory
    #[arg(short, long, default_value = "__snapshots__/.cas")]
    pub dir: String,

    #[command(subcommand)]
    pub action: TimelineAction,
}

#[derive(Debug, Subcommand)]
pub enum TimelineAction {
    /// Show historical observation commits for an endpoint
    Show {
        endpoint: String,
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },

    /// Compute full AST DiffReport between two historical moments in time
    Diff {
        endpoint: String,
        #[arg(long)]
        commit_a: String,
        #[arg(long)]
        commit_b: String,
    },
}

#[derive(Debug, Args)]
pub struct BlastRadiusArgs {
    /// Path to apisnap.toml configuration file
    #[arg(short, long, default_value = "apisnap.toml")]
    pub config: String,

    /// Target endpoint whose changes to analyze
    #[arg(short, long)]
    pub endpoint: String,
}

#[derive(Debug, Args)]
pub struct CaptureArgs {
    /// Address and port for the local reverse proxy to listen on
    #[arg(short, long, default_value = "127.0.0.1:9090")]
    pub proxy: String,

    /// Target upstream service URL
    #[arg(short, long, default_value = "http://127.0.0.1:8000")]
    pub target: String,

    /// Target snapshot directory
    #[arg(short, long, default_value = "__snapshots__")]
    pub snapshot_dir: String,
}

#[derive(Debug, Args)]
pub struct FuzzArgs {
    /// Path to apisnap.toml configuration file
    #[arg(short, long, default_value = "apisnap.toml")]
    pub config: String,

    /// Name of specific endpoint to fuzz (or all endpoints if omitted)
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

    /// Verify recorded snapshots or live endpoints against an existing OpenAPI specification file
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

    /// Query live endpoints in real time rather than reading stored snapshots
    #[arg(long)]
    pub live: bool,
}

#[derive(Debug, Args)]
pub struct CasArgs {
    /// CAS directory path
    #[arg(short, long, default_value = "__snapshots__/.cas")]
    pub dir: String,

    #[command(subcommand)]
    pub action: CasAction,
}

#[derive(Debug, Subcommand)]
pub enum CasAction {
    /// Print statistics on deduplication ratio and stored chunk count
    Stats,

    /// Reconstruct and print full JSON AST for a given 64-character hex NodeHash
    Inspect { hash: String },
}

#[derive(Debug, Args)]
pub struct SniffArgs {
    /// Target network port to capture (e.g. 80, 8080, 3000)
    #[arg(short, long, default_value = "8080")]
    pub port: u16,

    /// Snapshot output directory
    #[arg(short, long, default_value = "__snapshots__")]
    pub output_dir: String,

    /// Maximum number of packets to capture before stopping
    #[arg(short, long, default_value = "10")]
    pub count: usize,
}

#[derive(Debug, Args)]
pub struct ShadowArgs {
    /// Base URL of the baseline upstream service
    #[arg(long, default_value = "http://localhost:8080")]
    pub baseline: String,

    /// Base URL of the candidate upstream service
    #[arg(long, default_value = "http://localhost:8081")]
    pub candidate: String,

    /// Local port to listen for incoming shadow proxy traffic
    #[arg(short, long, default_value = "18000")]
    pub listen_port: u16,
}
