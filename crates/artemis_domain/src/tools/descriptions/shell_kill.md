Terminates a running shell job.

Use this when a background command should be stopped.

Inputs:
  - `job_id`: identifier returned by `{{tool_names.shell}}`.

Returns whether the termination request was successfully sent.
