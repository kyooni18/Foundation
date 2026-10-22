use clap::{Parser, Subcommand};
use prime_radiant::{App, domain::*};
use std::{io::Read, path::PathBuf};
#[derive(Parser)]
#[command(
    version,
    about = "Prime Radiant: authenticated transports and offline maintenance"
)]
struct Cli {
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Init,
    /// Queue any Phase 1/2 sources not yet assigned an extraction job.
    ExtractionBackfill,
    /// Run one durable job using OpenRouter credentials from the environment.
    ExtractionWork {
        #[arg(long)]
        model: String,
        #[arg(long)]
        reviewer: String,
        #[arg(long, default_value = "audited")]
        variant: String,
    },
    ExtractionInspect,
    ExtractionApprove {
        candidate: String,
        hash: String,
    },
    ExtractionRequest {
        id: SourceId,
        key: String,
    },
    Claims {
        #[arg(long)]
        principal: PrincipalKind,
    },
    ClaimStatus {
        id: String,
        event: String,
    },
    Migrate,
    /// Provision one independent credential; no token is printed.
    CredentialIssue {
        #[arg(long)]
        credentials: PathBuf,
        #[arg(long)]
        token_file: PathBuf,
        #[arg(long)]
        kind: PrincipalKind,
        #[arg(long)]
        label: Option<String>,
    },
    /// Own the DB and serve the authenticated loopback API.
    Serve {
        #[arg(long)]
        credentials: PathBuf,
        #[arg(long, default_value = "127.0.0.1:7823")]
        bind: std::net::SocketAddr,
        #[arg(long, requires = "extraction_reviewer")]
        extraction_model: Option<String>,
        #[arg(long, requires = "extraction_model")]
        extraction_reviewer: Option<String>,
    },
    /// Connect stdio MCP to the sole DB-owning server.
    Mcp {
        #[arg(long)]
        token_file: PathBuf,
        #[arg(long, default_value = "http://127.0.0.1:7823")]
        server: String,
    },
    Record {
        #[arg(long)]
        principal: PrincipalKind,
        #[arg(long)]
        label: Option<String>,
        #[arg(long)]
        session_id: Option<String>,
        /// Omit to read UTF-8 bytes from stdin.
        #[arg(long)]
        content: Option<String>,
        #[arg(long, default_value = "text/plain")]
        media_type: MediaType,
        #[arg(long)]
        authored_at: Option<Timestamp>,
        #[arg(long)]
        observed_at: Option<Timestamp>,
        #[arg(long)]
        artifact_ref: Option<String>,
        #[arg(long, default_value = "none")]
        retention_request: RetentionRequest,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    Get {
        #[arg(long)]
        principal: PrincipalKind,
        id: SourceId,
    },
    Lifecycle {
        #[arg(long)]
        principal: PrincipalKind,
        id: SourceId,
        event: LifecycleEvent,
        #[arg(long)]
        reason: Option<String>,
    },
    Check,
    Backup {
        path: PathBuf,
    },
    Restore {
        backup: PathBuf,
    },
}
fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
fn output(value: impl serde::Serialize) {
    println!(
        "{}",
        serde_json::to_string(&value).expect("serializable CLI output")
    );
}
fn run() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Command::CredentialIssue {
            credentials,
            token_file,
            kind,
            label,
        } => {
            prime_radiant::authentication::Credentials::issue(
                credentials,
                token_file,
                *kind,
                label.clone(),
            )?;
            output(serde_json::json!({"issued":true}));
            return Ok(());
        }
        Command::Mcp { token_file, server } => {
            let client = prime_radiant::transport::mcp::Client::new(server, token_file.clone())?;
            return tokio::runtime::Runtime::new()?.block_on(prime_radiant::transport::mcp::run(
                client,
                tokio::io::BufReader::new(tokio::io::stdin()),
                tokio::io::stdout(),
            ));
        }
        _ => (),
    }
    let path = cli
        .db
        .or_else(|| {
            std::env::var_os("HOME").map(|h| {
                PathBuf::from(h).join("Library/Application Support/PrimeRadiant/prime_radiant.db")
            })
        })
        .ok_or_else(|| invalid("db", "HOME unavailable; supply --db"))?;
    if let Command::Restore { backup } = cli.command {
        App::restore_from(backup, path)?;
        output(serde_json::json!({"restored":true}));
        return Ok(());
    }
    if let Command::Serve {
        credentials,
        bind,
        extraction_model,
        extraction_reviewer,
    } = cli.command
    {
        let credentials = prime_radiant::authentication::Credentials::open(credentials)?;
        let service = prime_radiant::api::Service::new(App::open(path)?, credentials);
        return tokio::runtime::Runtime::new()?.block_on(async move {
            let worker =
                if let (Some(model), Some(reviewer)) = (extraction_model, extraction_reviewer) {
                    let provider = prime_radiant::providers::OpenRouter::from_environment()
                        .map_err(|_| invalid("provider", "credential unavailable"))?;
                    let plan = prime_radiant::extraction::Plan {
                        prompt_version: None,
                        provider: "openrouter".into(),
                        variant: "audited".into(),
                        extractor: prime_radiant::providers::ModelConfig {
                            model,
                            timeout_seconds: 120,
                        },
                        reviewer: prime_radiant::providers::ModelConfig {
                            model: reviewer,
                            timeout_seconds: 120,
                        },
                    };
                    Some(service.start_extraction_worker(std::sync::Arc::new(provider), plan))
                } else {
                    None
                };
            let listener = prime_radiant::transport::http::bind(bind).await?;
            eprintln!("listening {}", listener.local_addr()?);
            axum::serve(listener, prime_radiant::transport::http::router(service))
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await?;
            if let Some(worker) = worker {
                worker.abort();
            }
            Ok(())
        });
    }
    let app = App::open(path)?;
    match cli.command {
        Command::Init | Command::Migrate => output(serde_json::json!({"schema_version":2})),
        Command::ExtractionBackfill => output(
            app.backfill_extraction(&Principal::for_local_cli(PrincipalKind::User, None, None))?,
        ),
        Command::ExtractionInspect => output(app.inspect_extraction(&Principal::for_local_cli(
            PrincipalKind::User,
            None,
            None,
        ))?),
        Command::ExtractionApprove { candidate, hash } => output(app.approve_extraction(
            &Principal::for_local_cli(PrincipalKind::User, None, None),
            &candidate,
            &hash,
        )?),
        Command::ExtractionRequest { id, key } => {
            app.request_extraction(
                &Principal::for_local_cli(PrincipalKind::User, None, None),
                id,
                &key,
            )?;
            output(true);
        }
        Command::Claims { principal } => {
            output(app.list_claims(&Principal::for_local_cli(principal, None, None))?)
        }
        Command::ClaimStatus { id, event } => {
            app.set_claim_status(
                &Principal::for_local_cli(PrincipalKind::User, None, None),
                &id,
                &event,
            )?;
            output(true);
        }
        Command::ExtractionWork {
            model,
            reviewer,
            variant,
        } => {
            let provider = prime_radiant::providers::OpenRouter::from_environment()
                .map_err(|_| invalid("provider", "credential unavailable"))?;
            let plan = prime_radiant::extraction::Plan {
                prompt_version: None,
                provider: "openrouter".into(),
                variant,
                extractor: prime_radiant::providers::ModelConfig {
                    model,
                    timeout_seconds: 120,
                },
                reviewer: prime_radiant::providers::ModelConfig {
                    model: reviewer,
                    timeout_seconds: 120,
                },
            };
            output(tokio::runtime::Runtime::new()?.block_on(app.work_once(&provider, &plan))?);
        }
        Command::Record {
            principal,
            label,
            session_id,
            content,
            media_type,
            authored_at,
            observed_at,
            artifact_ref,
            retention_request,
            idempotency_key,
        } => {
            let content = match content {
                Some(c) => c,
                None => {
                    let mut c = String::new();
                    std::io::stdin().read_to_string(&mut c)?;
                    c
                }
            };
            output(app.record_source(
                &Principal::for_local_cli(principal, label, session_id),
                RecordSource {
                    content,
                    media_type,
                    authored_at,
                    observed_at,
                    artifact_ref,
                    retention_request,
                    idempotency_key,
                },
            )?);
        }
        Command::Get { principal, id } => {
            output(app.get_source(&Principal::for_local_cli(principal, None, None), id)?)
        }
        Command::Lifecycle {
            principal,
            id,
            event,
            reason,
        } => {
            app.set_source_lifecycle(
                &Principal::for_local_cli(principal, None, None),
                id,
                event,
                reason,
            )?;
            output(serde_json::json!({"ok":true}));
        }
        Command::Check => {
            let report = app.verify_integrity()?;
            let ok = report.ok;
            output(report);
            if !ok {
                return Err(Error::IntegrityFailure("check failed".into()));
            }
        }
        Command::Backup { path } => {
            app.backup_to(path)?;
            output(serde_json::json!({"backed_up":true}));
        }
        Command::Restore { .. }
        | Command::Serve { .. }
        | Command::Mcp { .. }
        | Command::CredentialIssue { .. } => unreachable!(),
    }
    Ok(())
}
