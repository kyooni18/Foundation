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
    /// Embedding provider for semantic retrieval: local (default, runs on this machine),
    /// openrouter (opt-in cloud; sends Claim text to a third party) or off.
    #[arg(long, global = true, default_value = "local")]
    embedding: String,
    #[arg(long, global = true)]
    embedding_model: Option<String>,
    #[arg(long, global = true)]
    embedding_model_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Init,
    /// Initialize a separate private remote API key registry (never overwrites).
    RemoteKeyInit {
        path: PathBuf,
    },
    RemoteKeyList { path: PathBuf },
    /// Print one new remote API key ONCE; redirect stdout to a private file.
    RemoteKeyIssue {
        path: PathBuf,
        #[arg(long)]
        kind: PrincipalKind,
        #[arg(long)]
        label: String,
        #[arg(long, default_value = "memory.read memory.write memory.forget")]
        scopes: String,
        /// Unix timestamp, mandatory.
        #[arg(long)]
        expires: i64,
    },
    RemoteKeyRevoke {
        path: PathBuf,
        id: String,
    },
    /// Create a new persistent spending ledger; never resets an existing ledger.
    BudgetInit {
        path: PathBuf,
        #[arg(long, default_value_t = 4.5)]
        limit_usd: f64,
        #[arg(long)]
        prior_spend_usd: f64,
    },
    /// Queue any Phase 1/2 sources not yet assigned an extraction job.
    ExtractionBackfill,
    /// Run one durable job using OpenRouter credentials from the environment.
    ExtractionWork {
        #[arg(long, default_value = prime_radiant::providers::DEFAULT_GENERAL_MODEL)]
        model: String,
        #[arg(long)]
        reviewer: String,
        #[arg(long, default_value = "audited")]
        variant: String,
        #[arg(long)]
        prompt_version: Option<String>,
        #[arg(long)]
        max_output_tokens: Option<u32>,
        #[arg(long)]
        review_max_output_tokens: Option<u32>,
    },
    ExtractionInspect,
    /// Inspect exact source evidence and one quarantined proposal without approval.
    ExtractionReview {
        candidate: String,
    },
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
        /// Opt-in public MCP listener; private 0600 JSON configuration.
        #[arg(long)]
        remote_config: Option<PathBuf>,
        #[arg(long)]
        credentials: PathBuf,
        #[arg(long, default_value = "127.0.0.1:7823")]
        bind: std::net::SocketAddr,
        #[arg(long, requires = "extraction_reviewer")]
        extraction_model: Option<String>,
        #[arg(long, requires = "extraction_model")]
        extraction_reviewer: Option<String>,
        #[arg(long, requires = "extraction_model")]
        extraction_prompt_version: Option<String>,
        #[arg(long, requires = "extraction_model")]
        extraction_max_output_tokens: Option<u32>,
        #[arg(long, requires = "extraction_model")]
        extraction_review_max_output_tokens: Option<u32>,
    },
    /// Connect stdio MCP to the sole DB-owning server.
    Mcp {
        #[arg(long)]
        token_file: PathBuf,
        #[arg(long, default_value = "http://127.0.0.1:7823")]
        server: String,
        /// Expose all administrative/diagnostic tools (29) instead of the default four.
        #[arg(long)]
        full_tools: bool,
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
    /// Recompute derived validity and identity tables from authoritative rows.
    IntegrityRebuild,
    /// Run pending deterministic revalidation jobs; never calls a model.
    RevalidateWork {
        #[arg(long, default_value_t = 1000)]
        max_jobs: usize,
    },
    /// Run queued lexical and embedding jobs until the queue is idle (or limits are reached).
    IndexWork {
        #[arg(long, default_value = "all")]
        kind: String,
        #[arg(long, default_value_t = 100000)]
        max_rounds: usize,
    },
    /// Re-enqueue derived work missing for visible Claims (idempotent).
    IndexReconcile,
    /// Drop a derived index and let the queue rebuild it from authoritative Claims.
    IndexRebuild {
        /// lexical | vectors
        which: String,
    },
    /// Verify local model files against the pinned digests without loading weights.
    ModelCheck,
    /// Adaptive retrieval as the given principal.
    Search {
        #[arg(long)]
        principal: PrincipalKind,
        query: String,
        #[arg(long, default_value = "auto")]
        intent: String,
        #[arg(long, default_value = "auto")]
        depth: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long)]
        as_of: Option<String>,
        #[arg(long, default_value = "adaptive")]
        strategy: String,
    },
    QueueStatus,
    QueueJobs {
        #[arg(long)]
        state: Option<String>,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    QueueRetry {
        job: String,
    },
    /// Return jobs deferred because the provider was unavailable to the queue.
    QueueResume,
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
        Command::RemoteKeyInit { path } => {
            prime_radiant::transport::remote::key_init(path)?;
            return Ok(());
        }
        Command::RemoteKeyList { path } => {
            output(prime_radiant::transport::remote::key_list(path)?);
            return Ok(());
        }
        Command::RemoteKeyIssue {
            path,
            kind,
            label,
            scopes,
            expires,
        } => {
            println!(
                "{}",
                prime_radiant::transport::remote::key_issue(path, *kind, label, scopes, *expires)?
            );
            return Ok(());
        }
        Command::RemoteKeyRevoke { path, id } => {
            prime_radiant::transport::remote::key_revoke(path, id)?;
            return Ok(());
        }
        Command::BudgetInit {
            path,
            limit_usd,
            prior_spend_usd,
        } => {
            prime_radiant::budget::Budget::create(path, *limit_usd, *prior_spend_usd)
                .map_err(|_| invalid("budget", "ledger provisioning refused"))?;
            output(serde_json::json!({"created":true}));
            return Ok(());
        }
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
        Command::Mcp {
            token_file,
            server,
            full_tools,
        } => {
            let client = prime_radiant::transport::mcp::Client::new(server, token_file.clone())?;
            let client = if *full_tools {
                client.with_full_tools()
            } else {
                client
            };
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
    let policy = prime_radiant::Policy {
        embedding: prime_radiant::embedding::EmbeddingConfig::parse(
            &cli.embedding,
            cli.embedding_model.clone(),
            cli.embedding_model_dir.clone(),
        )
        .ok_or_else(|| invalid("embedding", "expected local, openrouter or off"))?,
        ..Default::default()
    };
    if let Command::Restore { backup } = cli.command {
        App::restore_from(backup, path)?;
        output(serde_json::json!({"restored":true}));
        return Ok(());
    }
    if let Command::Serve {
        remote_config,
        credentials,
        bind,
        extraction_model,
        extraction_reviewer,
        extraction_prompt_version,
        extraction_max_output_tokens,
        extraction_review_max_output_tokens,
    } = cli.command
    {
        let credentials = prime_radiant::authentication::Credentials::open(credentials)?;
        let service =
            prime_radiant::api::Service::new(App::open_with_policy(path, policy)?, credentials);
        return tokio::runtime::Runtime::new()?.block_on(async move {
            let worker =
                if let (Some(model), Some(reviewer)) = (extraction_model, extraction_reviewer) {
                    let provider = prime_radiant::providers::OpenRouter::from_environment()
                        .map_err(|_| invalid("provider", "credential unavailable"))?;
                    let plan = prime_radiant::extraction::Plan {
                        prompt_version: extraction_prompt_version,
                        provider: "openrouter".into(),
                        variant: "audited".into(),
                        extractor: prime_radiant::providers::ModelConfig {
                            max_output_tokens: extraction_max_output_tokens.or(Some(4096)),
                            model,
                            timeout_seconds: 120,
                        },
                        reviewer: prime_radiant::providers::ModelConfig {
                            max_output_tokens: extraction_review_max_output_tokens.or(Some(2048)),
                            model: reviewer,
                            timeout_seconds: 120,
                        },
                    };
                    plan.hash()?;
                    Some(service.start_extraction_worker(std::sync::Arc::new(provider), plan))
                } else {
                    None
                };
            let revalidation = service.start_revalidation_worker();
            let indexing = service.start_index_workers();
            let listener = prime_radiant::transport::http::bind(bind).await?;
            let remote = if let Some(path) = remote_config {
                let config = prime_radiant::transport::remote::Config::load(&path)?;
                let remote_listener = tokio::net::TcpListener::bind(config.bind).await?;
                let router =
                    prime_radiant::transport::remote::router(service.clone(), config).await?;
                Some(tokio::spawn(async move {
                    axum::serve(remote_listener, router).await
                }))
            } else {
                None
            };
            eprintln!("listening {}", listener.local_addr()?);
            axum::serve(listener, prime_radiant::transport::http::router(service))
                .with_graceful_shutdown(async {
                    #[cfg(unix)]
                    {
                        let mut terminate = tokio::signal::unix::signal(
                            tokio::signal::unix::SignalKind::terminate(),
                        )
                        .expect("SIGTERM handler");
                        tokio::select! {
                            _ = tokio::signal::ctrl_c() => {},
                            _ = terminate.recv() => {},
                        }
                    }
                    #[cfg(not(unix))]
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await?;
            if let Some(worker) = worker {
                worker.abort();
            }
            if let Some(remote) = remote {
                remote.abort();
            }
            revalidation.abort();
            indexing.iter().for_each(|h| h.abort());
            Ok(())
        });
    }
    let app = App::open_with_policy(path, policy)?;
    match cli.command {
        Command::Init | Command::Migrate => output(serde_json::json!({"schema_version":4})),
        Command::IntegrityRebuild => output(serde_json::json!({"claims":app.rebuild_derived(
            &Principal::for_local_cli(PrincipalKind::User, None, None),
        )?})),
        Command::RevalidateWork { max_jobs } => {
            let mut done = Vec::new();
            while done.len() < max_jobs {
                match app.revalidate_once("cli")? {
                    Some(result) => done.push(result),
                    None => break,
                }
            }
            output(done);
        }
        Command::ExtractionBackfill => output(
            app.backfill_extraction(&Principal::for_local_cli(PrincipalKind::User, None, None))?,
        ),
        Command::ExtractionInspect => output(app.inspect_extraction(&Principal::for_local_cli(
            PrincipalKind::User,
            None,
            None,
        ))?),
        Command::ExtractionReview { candidate } => output(app.extraction_review_packet(
            &Principal::for_local_cli(PrincipalKind::User, None, None),
            &candidate,
        )?),
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
            prompt_version,
            max_output_tokens,
            review_max_output_tokens,
        } => {
            let provider = prime_radiant::providers::OpenRouter::from_environment()
                .map_err(|_| invalid("provider", "credential unavailable"))?;
            let plan = prime_radiant::extraction::Plan {
                prompt_version,
                provider: "openrouter".into(),
                variant,
                extractor: prime_radiant::providers::ModelConfig {
                    max_output_tokens: max_output_tokens.or(Some(4096)),
                    model,
                    timeout_seconds: 120,
                },
                reviewer: prime_radiant::providers::ModelConfig {
                    max_output_tokens: review_max_output_tokens.or(Some(2048)),
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
        Command::IndexWork { kind, max_rounds } => {
            let owner = format!("cli-{}", std::process::id());
            let (mut lexical, mut embedded, mut failed, mut deferred) = (0, 0, 0, 0);
            for _ in 0..max_rounds {
                let mut busy = false;
                if kind == "all" || kind == "lexical" {
                    let r = app.process_lexical_once(&owner)?;
                    lexical += r.claims;
                    busy |= r.jobs + r.cancelled > 0;
                }
                if (kind == "all" || kind == "embed")
                    && let Some(provider) = app.embedder()
                {
                    let r = app.process_embeddings_once(provider.as_ref(), &owner)?;
                    embedded += r.claims;
                    failed += r.failed;
                    deferred += r.deferred;
                    busy |= r.jobs + r.cancelled > 0;
                    if r.failed + r.deferred > 0 {
                        break;
                    }
                }
                if !busy {
                    break;
                }
            }
            output(
                serde_json::json!({"lexical_indexed":lexical,"embedded":embedded,"failed":failed,"deferred":deferred}),
            );
        }
        Command::IndexReconcile => {
            let (l, v) = app.reconcile_indexing()?;
            output(serde_json::json!({"lexical_jobs":l,"embedding_jobs":v}));
        }
        Command::IndexRebuild { which } => {
            let user = Principal::for_local_cli(PrincipalKind::User, None, None);
            let n = match which.as_str() {
                "lexical" => app.rebuild_lexical(&user)?,
                "vectors" => app.rebuild_vectors(&user)?,
                _ => return Err(invalid("which", "expected lexical or vectors")),
            };
            output(serde_json::json!({"jobs_enqueued":n}));
        }
        Command::ModelCheck => {
            let id = app.policy_embedding_identity();
            output(
                serde_json::json!({"identity":id.as_ref().map(|i| serde_json::json!({"key":i.key,"provider":i.provider,"model":i.model,"revision":i.revision,"dimensions":i.dimensions,"local":i.local})),"ready":app.embedder().is_some()}),
            );
        }
        Command::Search {
            principal,
            query,
            intent,
            depth,
            limit,
            as_of,
            strategy,
        } => {
            fn parse<T: serde::de::DeserializeOwned>(field: &'static str, v: String) -> Result<T> {
                serde_json::from_value(serde_json::Value::String(v))
                    .map_err(|_| invalid(field, "unsupported value"))
            }
            let mut request = prime_radiant::retrieval::search::SearchRequest::new(query);
            request.intent = parse("intent", intent)?;
            request.depth = parse("depth", depth)?;
            request.strategy = parse("strategy", strategy)?;
            request.limit = limit;
            request.as_of = as_of;
            output(app.search(&Principal::for_local_cli(principal, None, None), request)?);
        }
        Command::QueueStatus => {
            output(app.queue_status(&Principal::for_local_cli(PrincipalKind::User, None, None))?)
        }
        Command::QueueJobs { state, limit } => output(app.queue_jobs(
            &Principal::for_local_cli(PrincipalKind::User, None, None),
            state.as_deref(),
            limit,
        )?),
        Command::QueueRetry { job } => output(
            serde_json::json!({"requeued":app.retry_job(&Principal::for_local_cli(PrincipalKind::User, None, None), &job)?}),
        ),
        Command::QueueResume => output(
            serde_json::json!({"resumed":app.resume_deferred(&Principal::for_local_cli(PrincipalKind::User, None, None), None)?}),
        ),
        Command::Restore { .. }
        | Command::RemoteKeyInit { .. }
        | Command::RemoteKeyList { .. }
        | Command::RemoteKeyIssue { .. }
        | Command::RemoteKeyRevoke { .. }
        | Command::Serve { .. }
        | Command::Mcp { .. }
        | Command::BudgetInit { .. }
        | Command::CredentialIssue { .. } => unreachable!(),
    }
    Ok(())
}
