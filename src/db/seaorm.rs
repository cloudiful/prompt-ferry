//! Native SeaORM bridges over the existing SQLx pools.
//!
//! The repository keeps one PostgreSQL pool and one standalone SQLite pool.
//! The native bridge adopts those existing handles instead of connecting
//! again, so no second pool, connection or shutdown duty is created: the
//! application stays the only pool owner, and the returned wrapper is never
//! closed explicitly because `DatabaseConnection::close` would close the
//! underlying pool.
//!
//! The error helpers keep the driver facts of a native error reachable
//! without message parsing: the SQLSTATE code and the constraint name are
//! read from the SQLx error the native layer owns.
//!
//! The transaction bridges run native statements on a SQLx connection the
//! caller already owns, so a native model route write joins the surrounding
//! multi-domain transaction instead of opening a second one.

use std::sync::Arc;

use async_trait::async_trait;
use sea_orm::sea_query::Values;
use sea_orm::sqlx::error::Error as SqlxError;
use sea_orm::sqlx::postgres::PgConnection;
use sea_orm::sqlx::sqlite::SqliteConnection;
use sea_orm::sqlx::{PgPool, SqlitePool};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, DbErr, ExecResult, QueryResult, RuntimeErr,
    SqlxPostgresConnector, SqlxSqliteConnector, Statement,
};
use sea_query_sqlx::SqlxValues;

/// Adopts the existing PostgreSQL pool in a native connection.
///
/// The returned wrapper shares the pool handle; dropping it does not close the
/// pool.
pub fn postgres_connection(pool: &PgPool) -> DatabaseConnection {
    SqlxPostgresConnector::from_sqlx_postgres_pool(pool.clone())
}

/// Adopts the existing standalone SQLite pool in a native connection.
///
/// The returned wrapper shares the pool handle, so the WAL, busy timeout and
/// foreign key settings of the existing pool stay in force.
pub fn sqlite_connection(pool: &SqlitePool) -> DatabaseConnection {
    SqlxSqliteConnector::from_sqlx_sqlite_pool(pool.clone())
}

/// Driver level facts of a native error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverErrorDetails {
    /// SQLSTATE code of the driver error, when the driver reports one.
    pub code: Option<String>,
    /// Name of the violated constraint or unique index, when reported.
    pub constraint: Option<String>,
}

/// Extracts the SQLSTATE code and the constraint name of a native error.
///
/// A native error keeps the driver error inside `DbErr::Exec`, `DbErr::Query`
/// or `DbErr::Conn`, so the facts are read from the owned driver error instead
/// of from the rendered message.
pub fn driver_error_details(error: &DbErr) -> Option<DriverErrorDetails> {
    let runtime = match error {
        DbErr::Exec(runtime) | DbErr::Query(runtime) | DbErr::Conn(runtime) => runtime,
        _ => return None,
    };
    let driver_error = match runtime {
        RuntimeErr::SqlxError(driver_error) => driver_error,
        _ => return None,
    };
    let database_error = match driver_error.as_ref() {
        SqlxError::Database(database_error) => database_error,
        _ => return None,
    };
    Some(DriverErrorDetails {
        code: database_error.code().map(|code| code.into_owned()),
        constraint: database_error.constraint().map(str::to_owned),
    })
}

/// Converts a native error into an application error while keeping the native
/// error owned by the error chain.
pub fn native_error(error: DbErr) -> anyhow::Error {
    anyhow::Error::new(error)
}

/// Declares a native SeaORM connection that runs its statements on an
/// existing SQLx connection.
///
/// The wrapper lets a native query join the transaction the caller already
/// opened, so a route write keeps the atomicity of the surrounding
/// multi-domain transaction instead of opening a second one.
///
/// The statement values are bound through `sea-query-sqlx`, the same
/// SeaQuery-to-SQLx adapter SeaORM itself uses, so every value variant the
/// entity layer can produce is bound by the driver library instead of a
/// hand-written mapping.
macro_rules! native_connection {
    ($name:ident, $connection:ty, $backend:expr) => {
        /// Native SeaORM connection bound to an existing SQLx connection.
        pub struct $name<'a> {
            connection: tokio::sync::Mutex<&'a mut $connection>,
        }

        impl<'a> $name<'a> {
            /// Binds the native connection to an existing SQLx connection.
            pub fn new(connection: &'a mut $connection) -> Self {
                Self {
                    connection: tokio::sync::Mutex::new(connection),
                }
            }
        }

        #[async_trait]
        impl ConnectionTrait for $name<'_> {
            fn get_database_backend(&self) -> DbBackend {
                $backend
            }

            async fn execute_raw(&self, stmt: Statement) -> Result<ExecResult, DbErr> {
                let mut guard = self.connection.lock().await;
                let query = sqlx::query_with(
                    sqlx::AssertSqlSafe(stmt.sql.as_str()),
                    SqlxValues(statement_values(&stmt)),
                );
                query
                    .execute(&mut **guard)
                    .await
                    .map(ExecResult::from)
                    .map_err(exec_error)
            }

            async fn execute_unprepared(&self, sql: &str) -> Result<ExecResult, DbErr> {
                let mut guard = self.connection.lock().await;
                sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()))
                    .execute(&mut **guard)
                    .await
                    .map(ExecResult::from)
                    .map_err(exec_error)
            }

            async fn query_one_raw(&self, stmt: Statement) -> Result<Option<QueryResult>, DbErr> {
                let mut guard = self.connection.lock().await;
                let query = sqlx::query_with(
                    sqlx::AssertSqlSafe(stmt.sql.as_str()),
                    SqlxValues(statement_values(&stmt)),
                );
                match query.fetch_optional(&mut **guard).await {
                    Ok(Some(row)) => Ok(Some(QueryResult::from(row))),
                    Ok(None) => Ok(None),
                    Err(error) => Err(query_error(error)),
                }
            }

            async fn query_all_raw(&self, stmt: Statement) -> Result<Vec<QueryResult>, DbErr> {
                let mut guard = self.connection.lock().await;
                let query = sqlx::query_with(
                    sqlx::AssertSqlSafe(stmt.sql.as_str()),
                    SqlxValues(statement_values(&stmt)),
                );
                query
                    .fetch_all(&mut **guard)
                    .await
                    .map(|rows| rows.into_iter().map(QueryResult::from).collect())
                    .map_err(query_error)
            }
        }
    };
}

native_connection!(PostgresNativeConnection, PgConnection, DbBackend::Postgres);
native_connection!(SqliteNativeConnection, SqliteConnection, DbBackend::Sqlite);

/// The bound values of a native statement, or an empty value list.
fn statement_values(stmt: &Statement) -> Values {
    stmt.values.clone().unwrap_or_else(|| Values(Vec::new()))
}

fn exec_error(error: SqlxError) -> DbErr {
    DbErr::Exec(RuntimeErr::SqlxError(Arc::new(error)))
}

fn query_error(error: SqlxError) -> DbErr {
    DbErr::Query(RuntimeErr::SqlxError(Arc::new(error)))
}
