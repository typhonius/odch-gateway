use sqlx::AnyPool;

/// Wrapper around `sqlx::AnyPool` that tracks the backend type.
#[derive(Clone)]
pub struct DbPool {
    pool: AnyPool,
    postgres: bool,
}

impl DbPool {
    /// Access the underlying sqlx pool.
    pub fn inner(&self) -> &AnyPool {
        &self.pool
    }

    /// Returns `true` when connected to PostgreSQL.
    pub fn is_postgres(&self) -> bool {
        self.postgres
    }
}

/// Create a connection pool from a database URL.
///
/// Accepts both SQLite (`sqlite:///path/to/db`) and PostgreSQL
/// (`postgres://user:pass@host:port/db`) URLs.
pub async fn create_pool(url: &str) -> Result<DbPool, sqlx::Error> {
    sqlx::any::install_default_drivers();
    let pool = sqlx::any::AnyPoolOptions::new()
        .max_connections(4)
        .connect(url)
        .await?;
    let postgres = url.starts_with("postgres");
    Ok(DbPool { pool, postgres })
}

#[cfg(test)]
pub(crate) async fn create_test_pool() -> DbPool {
    sqlx::any::install_default_drivers();
    // In-memory SQLite needs max_connections=1 so all queries share one DB.
    let pool = sqlx::any::AnyPoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("test pool");
    DbPool {
        pool,
        postgres: false,
    }
}
