Executes a shell command synchronously and returns output immediately. Use this for
short-lived commands that complete in seconds: DNS lookups, HTTP checks, git queries,
file listings, build checks, etc.

CRITICAL: Do NOT use `cd` commands in the command string. Always use the `cwd` parameter instead.

IMPORTANT: Choose the right tool:
- Use `{{tool_names.shell_exec}}` (this tool) for commands that finish quickly (<30s):
  nslookup, host, dig, curl, ping, git status, ls, cargo check, npm list, etc.
- Use `{{tool_names.shell}}` for genuinely long-running background tasks that take
  minutes: nmap full scans, npm run dev, cargo build --release, docker compose up, etc.

Before executing, follow these steps:

1. Verify the working directory and quoting:
   - Always quote paths containing spaces with double quotes.
   - Use the `cwd` parameter instead of `cd`.

2. Execution rules:
   - Blocks until the command completes and returns full stdout/stderr and exit code.
   - Do NOT chain blocking commands with `sleep` — just run them directly.
   - If several commands must run sequentially, combine with `&&`.

Usage notes:
  - It is very helpful to write a clear, concise description in 5-10 words.
  - If the output exceeds {{config.stdoutMaxPrefixLength}} prefix lines or {{config.stdoutMaxSuffixLength}}
    suffix lines, or if a line exceeds {{config.stdoutMaxLineLength}} characters, it will be truncated
    and the full output will be written to a temporary file.
  - Do not use this tool for file operations — use the dedicated fs tools instead.
  - Do not use `find`, `grep`, `cat`, `head`, `tail` unless truly necessary; prefer
    `{{tool_names.fs_search}}` and `{{tool_names.read}}`.

Good examples:
  - nslookup scanme.nmap.org
  - host scanme.nmap.org
  - dig +short google.com
  - curl -I https://example.com
  - git log --oneline -5
  - cargo check 2>&1

Bad examples (use `{{tool_names.shell}}` instead):
  - npm run dev        (long-running server)
  - nmap -A -T4 host  (full port scan, takes minutes)
  - docker compose up  (persistent service)
