use apisnap::cli::{
    handle_approve_diff, handle_blast_radius, handle_capture, handle_cas, handle_fuzz,
    handle_import, handle_init, handle_openapi_generate, handle_openapi_verify, handle_record,
    handle_review, handle_shadow, handle_sniff, handle_test, handle_timeline, Cli, Commands,
    OpenApiActions,
};
use clap::Parser;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init(args) => handle_init(&args.output),
        Commands::Import(args) => handle_import(&args),
        Commands::Record(args) => {
            handle_record(
                &args.config,
                args.endpoint.as_deref(),
                args.concurrency,
                args.cas,
                args.learn,
            )
            .await
        }
        Commands::Test(args) => {
            handle_test(
                &args.config,
                args.endpoint.as_deref(),
                args.concurrency,
                args.ci,
                args.pr_comment,
                args.baseline.as_deref(),
                args.candidate.as_deref(),
            )
            .await
        }
        Commands::Review(args) => handle_review(&args.config, args.endpoint.as_deref()).await,
        Commands::ApproveDiff(args) => handle_approve_diff(&args),
        Commands::Timeline(args) => handle_timeline(&args),
        Commands::BlastRadius(args) => handle_blast_radius(&args).await,
        Commands::Capture(args) => handle_capture(&args).await,
        Commands::Fuzz(args) => handle_fuzz(&args.config, args.endpoint.as_deref()).await,
        Commands::Openapi(sub) => match sub.action {
            OpenApiActions::Generate(args) => handle_openapi_generate(&args.config, &args.output),
            OpenApiActions::Verify(args) => {
                handle_openapi_verify(&args.config, &args.spec, args.live).await
            }
        },
        Commands::Cas(args) => handle_cas(&args),
        Commands::Sniff(args) => handle_sniff(&args).await,
        Commands::Shadow(args) => handle_shadow(&args).await,
    };

    if let Err(err) = result {
        eprintln!("{err}");
        std::process::exit(err.exit_code());
    }
}
