//! coach-pyo3: the Python-facing ingest SDK.
//!
//! Abi3 wheel (`abi3-py313`): built once for 3.13, runs on every later
//! interpreter. The Python layer (`coach_spacy`) uses this SDK to create runs
//! and push metric batches without going through HTTP when embedded locally,
//! and falls back to the HTTP API when the SDK is absent.

use pyo3::prelude::*;

#[pyfunction]
fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[pyfunction]
/// Placeholder ingest: lists metric point dicts in the wire format the API
/// server consumes. Real transport (HTTP or shared memory) lands with the
/// ingest feature; validation happens in coach-core.
fn metrics_batch_json(points: Vec<Bound<PyAny>>) -> PyResult<String> {
    let out: Vec<String> = points.iter().map(|p| p.to_string()).collect();
    Ok(format!("[{}]", out.join(",")))
}

#[pymodule]
fn coach_sdk(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(metrics_batch_json, m)?)?;
    Ok(())
}
