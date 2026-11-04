use anyhow::{bail, Result};
use streaming_player::{auth, build_state, chat, media::ytdlp, service, tokens, Config};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

const USAGE: &str = "Usage:
  streaming-player                                  start the server
  streaming-player reset-password <user> <password> reset a web user's password
  streaming-player update-ytdlp                     update yt-dlp to the latest stable release
  streaming-player openapi                          print the OpenAPI document

Settings come from SP_* environment variables, see README.";

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn")),
        )
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let config = Config::from_env()?;
    match args.first().map(String::as_str) {
        None => serve(config).await,
        Some("reset-password") => {
            let (Some(user), Some(password)) = (args.get(1), args.get(2)) else {
                bail!("{USAGE}");
            };
            reset_password(config, user, password).await
        }
        Some("update-ytdlp") => {
            println!("{}", ytdlp::update(&config.ytdlp).await?);
            Ok(())
        }
        Some("openapi") => {
            let (_, doc) = streaming_player::routes::openapi_router().split_for_parts();
            println!("{}", doc.to_pretty_json()?);
            Ok(())
        }
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => bail!("unknown command: {other}\n\n{USAGE}"),
    }
}

async fn serve(config: Config) -> Result<()> {
    tracing::info!(
        "streaming-player v{} | database {} | public URL {}",
        env!("CARGO_PKG_VERSION"),
        config.database_path.display(),
        config.public_url
    );
    match ytdlp::version(&config.ytdlp).await {
        Ok(v) => tracing::info!("yt-dlp {v}"),
        Err(e) => tracing::warn!("yt-dlp is not available, only YouTube embeds will work: {e}"),
    }
    let bind = config.bind.clone();
    let state = build_state(config).await?;
    chat::restart(&state);

    let listener = TcpListener::bind(&bind).await?;
    tracing::info!("listening on http://{}", listener.local_addr()?);
    axum::serve(listener, service(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    tracing::info!("stopped");
    Ok(())
}

async fn reset_password(config: Config, user: &str, password: &str) -> Result<()> {
    if password.chars().count() < 8 {
        bail!("the password needs at least 8 characters");
    }
    let state = build_state(config).await?;
    let hash = auth::hash_password(password.to_string()).await?;
    let id: Option<i64> =
        sqlx::query_scalar("UPDATE users SET password_hash = ? WHERE username = ? RETURNING id")
            .bind(hash)
            .bind(user)
            .fetch_optional(&state.db)
            .await?;
    let Some(id) = id else {
        bail!("no such user: {user}");
    };
    tokens::revoke_all_for_user(&state.db, id).await?;
    println!("password reset for {user}; their sessions and tokens were signed out");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
