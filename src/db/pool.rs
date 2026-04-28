use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// Wrapper around `sqlx::PgPool`.
#[derive(Clone)]
pub struct DbPool {
    pool: PgPool,
}

impl DbPool {
    /// Access the underlying sqlx pool.
    pub fn inner(&self) -> &PgPool {
        &self.pool
    }
}

/// Create a PostgreSQL connection pool and run migrations.
pub async fn create_pool(url: &str) -> Result<DbPool, sqlx::Error> {
    let pool = PgPoolOptions::new().max_connections(8).connect(url).await?;

    // Run migrations
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|e| sqlx::Error::Configuration(format!("Migration failed: {}", e).into()))?;

    tracing::info!("Database migrations applied successfully");
    Ok(DbPool { pool })
}
