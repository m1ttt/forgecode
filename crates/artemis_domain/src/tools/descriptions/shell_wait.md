Waits for a shell job to complete.

Use this after starting a command with `{{tool_names.shell}}` when you explicitly need final output.
Do not call this immediately after every background command by default. Prefer returning control to the user first, then wait only when the result is actually needed.

Inputs:
  - `job_id`: identifier returned by `{{tool_names.shell}}`.
  - `timeout_ms` (optional): maximum milliseconds to wait before returning.
  - `keep_ansi` (optional): if true, preserves ANSI escape sequences.

Returns the latest stdout/stderr buffers, command, running flag, and exit code (when available).
