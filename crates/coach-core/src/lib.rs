//! coach-core: domain model, storage and query engine for AI Small Modell Coach.
//!
//! The crate is intentionally backend-agnostic: the storage traits in
//! [`storage`] define the stable contract, with an SQLite backend
//! (`sqlite` feature) used by the server, an in-memory reference
//! implementation behind the default `inmemory` feature, and a Parquet/DuckDB
//! analytics layer kept as a documented escape hatch behind the optional
//! `duckdb-backend` feature.

pub mod error;
pub mod model;
pub mod query;
#[cfg(feature = "sqlite")]
pub mod sqlite_backend;
pub mod storage;

pub use error::{CoreError, Result};
