Starts a shell command as an **asynchronous background job**. Returns immediately with a
`job_id`. The command keeps running in the background. The `cwd` parameter sets the
working directory; if omitted, defaults to `{{env.cwd}}`.

CRITICAL: Do NOT use `cd` commands. Always use the `cwd` parameter instead.

IMPORTANT: Choose the right tool:
- Use `{{tool_names.shell_exec}}` for commands that complete in seconds (nslookup, host,
  dig, curl, git status, cargo check, ls, ping, etc.) — it blocks and returns output
  immediately, with no background job overhead.
- Use `{{tool_names.shell}}` (this tool) **only** for genuinely long-running background
  tasks that take minutes: nmap full port scans, `npm run dev`, `cargo build --release`,
  `docker compose up`, `pytest` on a large suite, etc.

IMPORTANT: This tool is for terminal operations like git, npm, docker, etc. DO NOT use it
for file operations (reading, writing, editing, searching) — use the fs tools instead.

Before executing, follow these steps:

1. Directory Verification:
   - If the command creates new directories/files, first verify the parent dir exists.
   - Always quote paths that contain spaces with double quotes.

2. Command Start:
   - This tool returns immediately with a `job_id`.
   - After receiving the `job_id`, stop issuing additional shell commands in the same turn
     unless you need to cancel it immediately.
   - Return control to the user so the conversation continues while the job runs.
   - Use `{{tool_names.shell_wait}}` to block for completion when the result is needed.
   - Use `{{tool_names.shell_poll}}` to inspect progress without blocking.
   - Use `{{tool_names.shell_kill}}` to terminate a running job.
   - Do not start a second shell job while one is still running. Reuse the same `job_id`
     with poll/wait/kill until it reaches a terminal state.

Usage notes:
  - The command argument is required.
  - Write a clear, concise description of what this command does in 5-10 words.
  - If the output exceeds {{config.stdoutMaxPrefixLength}} prefix lines or {{config.stdoutMaxSuffixLength}}
    suffix lines, or if a line exceeds {{config.stdoutMaxLineLength}} characters, it will be
    truncated; the full output is written to a temporary file you can read with the read tool.
  - Do NOT use `head`, `tail`, or other truncation — run the command directly.
  - Do not use `{{tool_names.shell}}` with `find`, `grep`, `cat`, `head`, `tail`, `sed`,
    `awk`, or `echo` unless truly necessary; prefer `{{tool_names.fs_search}}` and
    `{{tool_names.read}}`.
  - When issuing commands:
    - Keep only one active shell job at a time.
    - If a job is still running, use `{{tool_names.shell_poll}}` or `{{tool_names.shell_wait}}`
      instead of launching another command.
    - Do NOT issue helper shell commands such as `wait`, `sleep`, `jobs`, `ps`, or a
      repeated copy of the same command merely to monitor a background shell job.
    - Combine sequential commands into a single call with `&&` when each depends on the
      previous. Use `;` only when you don't care if earlier commands fail.
    - Do NOT use newlines to separate commands (newlines are ok inside quoted strings).
  - DO NOT use `cd <dir> && <command>`. Use the `cwd` parameter instead.

Good examples (correct use of background shell):
  - nmap -A -T4 -p- 45.33.32.156       (full scan, can take minutes)
  - npm run dev                          (long-running dev server)
  - cargo build --release               (slow release build)

Bad examples (use `{{tool_names.shell_exec}}` instead):
  - nslookup scanme.nmap.org            (resolves in <1s)
  - host scanme.nmap.org                (resolves in <1s)
  - curl -I https://example.com         (HTTP check, fast)
  - git status                          (instant)

Returns a `job_id` for subsequent poll/wait/kill operations.
