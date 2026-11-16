export default function Images() {
  return (
    <div>
      <div className="page-header">
        <h1>Images</h1>
        <p>
          Image artifacts (training-time displaCy renders, PNG exports)
          grouped by run and session.
        </p>
      </div>
      <div className="empty-state">
        <h3>No images yet</h3>
        <p>
          Set <code>viz_path</code> and <code>model_log_interval</code> in your
          training config and the logger uploads sample renders while training.
          They appear here grouped by run.
        </p>
      </div>
    </div>
  );
}
