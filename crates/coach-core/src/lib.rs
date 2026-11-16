//! coach-core: domain model, storage and query engine for AI Small Modell Coach.
//!
//! The crate is intentionally backend-agnostic: the storage traits in
//! [`storage`] define the stable contract, with an in-memory reference
//! implementation behind the default `inmemory` feature and Parquet/DuckDB
//! backends planned behind the `duckdb-backend` feature.

pub mod error;
pub mod model;
pub mod query;
pub mod storage;

pub use error::{CoreError, Result};
