//! Database module - connection pool and migration runner

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::sync::Mutex;

static POOL: Mutex<Option<PgPool>> = Mutex::new(None);

pub fn get_pool() -> PgPool {
    POOL.lock()
        .unwrap()
        .clone()
        .expect("Database pool not initialized. Call init_db() first.")
}

pub async fn init_db(database_url: &str) -> anyhow::Result<PgPool> {
    // 每次调用新建连接池并返回：调用方（含测试）持有自己的池，避免跨 tokio runtime
    // 复用池导致获取到失效连接（集成测试曾因此 pool timed out）。
    // 全局槽位只记录首个池，供 db::get_pool() 在进程内共享（如 main/seed）。
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;

    tracing::info!("Connected to database");
    run_migrations(&pool).await?;

    let mut guard = POOL.lock().unwrap();
    if guard.is_none() {
        *guard = Some(pool.clone());
    }

    Ok(pool)
}

async fn run_migrations(pool: &PgPool) -> anyhow::Result<()> {
    let migration_dir = std::env::var("MIGRATIONS_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations"));

    if !migration_dir.exists() {
        tracing::warn!("Migrations directory not found: {:?}", migration_dir);
        return Ok(());
    }

    let mut entries: Vec<_> = std::fs::read_dir(&migration_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "sql"))
        .collect();

    entries.sort_by_key(|e| e.file_name());

    for entry in &entries {
        let path = entry.path();
        let sql = std::fs::read_to_string(&path)?;
        tracing::info!(
            "Running migration: {}",
            path.file_name().unwrap().to_string_lossy()
        );

        sqlx::raw_sql(&sql)
            .execute(pool)
            .await
            .map_err(|e| anyhow::anyhow!("Migration '{}' failed: {}", path.display(), e))?;
    }

    tracing::info!("All migrations applied successfully");
    Ok(())
}
