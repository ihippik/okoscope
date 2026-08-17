use server::database::{MIGRATOR, REQUIRED_MIGRATION, migrate, verify_schema};

#[sqlx::test]
#[ignore = "requires a PostgreSQL server with DATABASE_URL"]
async fn migration_only_initializes_a_fresh_database(pool: sqlx::PgPool) {
    let report = migrate(&pool).await.expect("fresh migration succeeds");

    assert_eq!(report.required, REQUIRED_MIGRATION);
    assert_eq!(report.applied, REQUIRED_MIGRATION);
    verify_schema(&pool).await.expect("schema is current");
}

#[sqlx::test(migrator = "MIGRATOR")]
#[ignore = "requires a PostgreSQL server with DATABASE_URL"]
async fn migration_only_is_idempotent_when_current(pool: sqlx::PgPool) {
    let first = migrate(&pool).await.expect("current migration succeeds");
    let second = migrate(&pool).await.expect("retry succeeds");

    assert_eq!(first, second);
    assert_eq!(second.applied, REQUIRED_MIGRATION);
}

#[sqlx::test(migrator = "MIGRATOR")]
#[ignore = "requires a PostgreSQL server with DATABASE_URL"]
async fn migration_only_reports_a_failed_migration(pool: sqlx::PgPool) {
    sqlx::query("UPDATE _sqlx_migrations SET success = false WHERE version = $1")
        .bind(REQUIRED_MIGRATION)
        .execute(&pool)
        .await
        .expect("mark migration failed");

    migrate(&pool).await.expect_err("dirty migration must fail");
    verify_schema(&pool)
        .await
        .expect_err("failed required migration must not be ready");
}

#[sqlx::test(migrator = "MIGRATOR")]
#[ignore = "requires a PostgreSQL server with DATABASE_URL"]
async fn migration_only_can_retry_after_failure_is_repaired(pool: sqlx::PgPool) {
    sqlx::query("UPDATE _sqlx_migrations SET success = false WHERE version = $1")
        .bind(REQUIRED_MIGRATION)
        .execute(&pool)
        .await
        .expect("mark migration failed");
    migrate(&pool).await.expect_err("dirty migration must fail");

    sqlx::query("UPDATE _sqlx_migrations SET success = true WHERE version = $1")
        .bind(REQUIRED_MIGRATION)
        .execute(&pool)
        .await
        .expect("repair migration history");
    let report = migrate(&pool).await.expect("retry succeeds after repair");

    assert_eq!(report.applied, REQUIRED_MIGRATION);
}
