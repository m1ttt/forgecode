Polls the latest state of a running shell job.

Use this when you need intermediate output without blocking the turn.
Prefer this over ad-hoc shell commands such as `sleep`, `wait`, `jobs`, or rerunning the original command.

Inputs:
  - `job_id`: identifier returned by `{{tool_names.shell}}`.
  - `keep_ansi` (optional): if true, preserves ANSI escape sequences.

Returns the current stdout/stderr buffers, command, running flag, and exit code (when available).
