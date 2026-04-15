#!/usr/bin/env zsh

# Configuration variables for artemis plugin
# Using typeset to keep variables local to plugin scope and prevent public exposure

typeset -h _ARTEMIS_BIN="${ARTEMIS_BIN:-artemis}"
typeset -h _ARTEMIS_CONVERSATION_PATTERN=":"
typeset -h _ARTEMIS_MAX_COMMIT_DIFF="${ARTEMIS_MAX_COMMIT_DIFF:-100000}"
typeset -h _ARTEMIS_DELIMITER='\s\s+'
typeset -h _ARTEMIS_PREVIEW_WINDOW="--preview-window=bottom:75%:wrap:border-sharp"

# Detect bat command - use bat if available, otherwise fall back to cat
if command -v bat &>/dev/null; then
    typeset -h _ARTEMIS_CAT_CMD="bat --color=always --style=numbers,changes --line-range=:500"
else
    typeset -h _ARTEMIS_CAT_CMD="cat"
fi

# Commands cache - loaded lazily on first use
typeset -h _ARTEMIS_COMMANDS=""

# Hidden variables to be used only via the ArtemisCLI
typeset -h _ARTEMIS_CONVERSATION_ID
typeset -h _ARTEMIS_ACTIVE_AGENT

# Previous conversation ID for :conversation - (like cd -)
typeset -h _ARTEMIS_PREVIOUS_CONVERSATION_ID

# Session-scoped model and provider overrides (set via :model / :m).
# When non-empty, these are passed as --model / --provider to every artemis
# invocation for the lifetime of the current shell session.
typeset -h _ARTEMIS_SESSION_MODEL
typeset -h _ARTEMIS_SESSION_PROVIDER

# Session-scoped reasoning effort override (set via :reasoning-effort / :re).
# When non-empty, exported as ARTEMIS_REASONING__EFFORT for every artemis invocation.
typeset -h _ARTEMIS_SESSION_REASONING_EFFORT

# Terminal context capture settings
# Master switch for terminal context capture (preexec/precmd hooks)
typeset -h _ARTEMIS_TERM_ENABLED="${ARTEMIS_TERM_ENABLED:-true}"
# Maximum number of commands to keep in the ring buffer (metadata: cmd + exit code)
typeset -h _ARTEMIS_TERM_MAX_COMMANDS="${ARTEMIS_TERM_MAX_COMMANDS:-5}"
# OSC 133 semantic prompt marker emission: "auto", "on", or "off"
typeset -h _ARTEMIS_TERM_OSC133="${ARTEMIS_TERM_OSC133:-auto}"
# Ring buffer arrays for context capture
typeset -ha _ARTEMIS_TERM_COMMANDS=()
typeset -ha _ARTEMIS_TERM_EXIT_CODES=()
typeset -ha _ARTEMIS_TERM_TIMESTAMPS=()
