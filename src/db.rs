use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

pub async fn create_pool(database_url: &str) -> PgPool {
    let max = std::env::var("DATABASE_MAX_CONNECTIONS")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(32);
    let min = std::env::var("DATABASE_MIN_CONNECTIONS")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(4);

    PgPoolOptions::new()
        .max_connections(max)
        .min_connections(min)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .idle_timeout(std::time::Duration::from_secs(600))
        .connect(database_url)
        .await
        .expect("Failed to connect to PostgreSQL")
}

pub async fn run_migrations(pool: &PgPool) {
    sqlx::raw_sql(include_str!("../migrations/001_initial_schema.sql"))
        .execute(pool)
        .await
        .expect("Failed to run migration 001");

    sqlx::raw_sql(include_str!("../migrations/002_code_batches.sql"))
        .execute(pool)
        .await
        .expect("Failed to run migration 002");

    sqlx::raw_sql(include_str!("../migrations/003_redemptions.sql"))
        .execute(pool)
        .await
        .expect("Failed to run migration 003");

    sqlx::raw_sql(include_str!("../migrations/004_pending_redemptions.sql"))
        .execute(pool)
        .await
        .expect("Failed to run migration 004");

    sqlx::raw_sql(include_str!("../migrations/005_temporary_roles.sql"))
        .execute(pool)
        .await
        .expect("Failed to run migration 005");
}
