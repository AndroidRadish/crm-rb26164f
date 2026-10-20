//! RB-26164F - 10人团队轻量级CRM系统

use axum::Router;
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::EnvFilter;

pub mod db;
pub mod handlers;
pub mod models;
pub mod routes;
pub mod seed;
pub mod services;

#[derive(Parser)]
#[command(name = "crm-rb26164f", about = "RB-26164F - 轻量级CRM系统")]
struct Cli {
    #[command(subcommand)]
    command: Option<CliCommand>,
}

#[derive(Subcommand)]
enum CliCommand {
    /// Start the web server (default)
    Serve,
    /// 清库并写入种子数据（开发/演示）
    Seed,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/crm_rb26164f".to_string());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(9092);

    let cli = Cli::parse();
    let cmd = cli.command.unwrap_or(CliCommand::Serve);

    match cmd {
        CliCommand::Seed => {
            seed::run_seed(&database_url).await?;
            Ok(())
        }
        CliCommand::Serve => {
            // 安全（C-P0-1）：未配置 JWT_SECRET 直接拒绝启动（fail-fast），
            // 不再回退到源码公开默认密钥。放在连库之前，保证配置缺失时立即失败。
            services::auth::ensure_jwt_secret_configured()?;
            let pool = db::init_db(&database_url).await?;
            let cors = CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any);
            let app = Router::new()
                .nest("/api", routes::router())
                .layer(cors)
                .with_state(pool);
            let addr = SocketAddr::from(([0, 0, 0, 0], port));
            tracing::info!("CRM server listening on {}", addr);
            let listener = tokio::net::TcpListener::bind(addr).await?;
            axum::serve(listener, app).await?;
            Ok(())
        }
    }
}
