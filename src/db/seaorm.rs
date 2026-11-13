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

use sea_orm::sqlx::error::Error as SqlxError;
use sea_orm::sqlx::{PgPool, SqlitePool};
use sea_orm::{DatabaseConnection, DbErr, RuntimeErr, SqlxPostgresConnector, SqlxSqliteConnector};

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
