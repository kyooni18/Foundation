use clap::{Parser, Subcommand};
use prime_radiant::{App, domain::*};
use std::{io::Read, path::PathBuf};
#[derive(Parser)]
#[command(
    version,
    about = "Prime Radiant local persistence CLI (explicit local principal, no authentication)"
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
    Migrate,
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
    let app = App::open(path)?;
    match cli.command {
        Command::Init | Command::Migrate => output(serde_json::json!({"schema_version":1})),
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
        Command::Restore { .. } => unreachable!(),
    }
    Ok(())
}
