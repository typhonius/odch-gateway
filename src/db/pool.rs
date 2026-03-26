use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

pub type DbPool = Pool<SqliteConnectionManager>;

/// Create a read-only SQLite connection pool.
///
/// The bot process owns writes; we only read.
pub fn create_pool(path: &str) -> Result<DbPool, r2d2::Error> {
    let manager =
        SqliteConnectionManager::file(path).with_flags(rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY);
    Pool::builder().max_size(4).build(manager)
}

#[cfg(test)]
pub(crate) fn create_test_pool() -> DbPool {
    // In-memory DB needs read-write so we can create tables and seed data.
    let manager = SqliteConnectionManager::memory();
    Pool::builder()
        .max_size(1)
        .build(manager)
        .expect("test pool")
}
