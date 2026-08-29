use apisnap::cli::{
    handle_fuzz, handle_init, handle_openapi_generate, handle_openapi_verify, handle_record,
    handle_review, handle_test, Cli, Commands, OpenApiActions,
};
use clap::Parser;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init(args) => handle_init(&args.output),
        Commands::Record(args) => {
            handle_record(&args.config, args.endpoint.as_deref(), args.concurrency).await
        }
        Commands::Test(args) => {
            handle_test(
                &args.config,
                args.endpoint.as_deref(),
                args.concurrency,
                args.ci,
                args.pr_comment,
            )
            .await
        }
        Commands::Review(args) => {
            handle_review(&args.config, args.endpoint.as_deref()).await
        }
        Commands::Fuzz(args) => {
            handle_fuzz(&args.config, args.endpoint.as_deref()).await
        }
        Commands::Openapi(sub) => match sub.action {
            OpenApiActions::Generate(args) => handle_openapi_generate(&args.config, &args.output),
            OpenApiActions::Verify(args) => {
                handle_openapi_verify(&args.config, &args.spec, args.live).await
            }
        },
    };

    if let Err(err) = result {
        eprintln!("{err}");
        std::process::exit(err.exit_code());
    }
}
