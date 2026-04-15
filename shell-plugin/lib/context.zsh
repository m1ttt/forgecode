#!/usr/bin/env zsh

# Terminal context capture for artemis plugin
#
# Provides three layers of terminal context:
# 1. preexec/precmd hooks: ring buffer of recent commands + exit codes
# 2. OSC 133 emission: semantic terminal markers for compatible terminals
# 3. Terminal-specific output capture: Kitty > WezTerm > tmux
#
# Context is organized by command blocks: each command's metadata and its
# full output are grouped together, using the known command strings from
# the ring buffer to detect boundaries in the terminal scrollback.

# ---------------------------------------------------------------------------
# OSC 133 helpers
# ---------------------------------------------------------------------------

# Determines whether OSC 133 semantic markers should be emitted.
# Auto-detection is conservative: only emit for terminals known to support it
# to avoid garbled output in unsupported terminals.
# The detection result is cached per session in _ARTEMIS_TERM_OSC133_CACHED
# ("1" = emit, "0" = don't emit) to avoid repeated detection overhead.
typeset -g _ARTEMIS_TERM_OSC133_CACHED=""
function _artemis_osc133_should_emit() {
    if [[ -n "$_ARTEMIS_TERM_OSC133_CACHED" ]]; then
        [[ "$_ARTEMIS_TERM_OSC133_CACHED" == "1" ]] && return 0 || return 1
    fi
    case "$_ARTEMIS_TERM_OSC133" in
        on)  _ARTEMIS_TERM_OSC133_CACHED="1"; return 0 ;;
        off) _ARTEMIS_TERM_OSC133_CACHED="0"; return 1 ;;
        auto)
            # Kitty sets KITTY_PID
            if [[ -n "${KITTY_PID:-}" ]]; then _ARTEMIS_TERM_OSC133_CACHED="1"; return 0; fi
            # Detect by TERM_PROGRAM
            case "${TERM_PROGRAM:-}" in
                WezTerm|iTerm.app|vscode) _ARTEMIS_TERM_OSC133_CACHED="1"; return 0 ;;
            esac
            # Foot terminal
            if [[ "${TERM:-}" == "foot"* ]]; then _ARTEMIS_TERM_OSC133_CACHED="1"; return 0; fi
            # Ghostty
            if [[ "${TERM_PROGRAM:-}" == "ghostty" ]]; then _ARTEMIS_TERM_OSC133_CACHED="1"; return 0; fi
            # Unknown terminal: don't emit
            _ARTEMIS_TERM_OSC133_CACHED="0"
            return 1
            ;;
        *)   _ARTEMIS_TERM_OSC133_CACHED="0"; return 1 ;;
    esac
}

# Emits an OSC 133 marker if the terminal supports it.
# Usage: _artemis_osc133_emit "A"  or  _artemis_osc133_emit "D;0"
function _artemis_osc133_emit() {
    _artemis_osc133_should_emit || return 0
    printf '\e]133;%s\a' "$1"
}

# ---------------------------------------------------------------------------
# preexec / precmd hooks
# ---------------------------------------------------------------------------

# Ring buffer storage uses parallel arrays declared in config.zsh:
#   _ARTEMIS_TERM_COMMANDS, _ARTEMIS_TERM_EXIT_CODES, _ARTEMIS_TERM_TIMESTAMPS
# Pending command state:
typeset -g _ARTEMIS_TERM_PENDING_CMD=""
typeset -g _ARTEMIS_TERM_PENDING_TS=""

# Called before each command executes.
# Records the command text and timestamp, emits OSC 133 B+C markers.
function _artemis_context_preexec() {
    [[ "$_ARTEMIS_TERM_ENABLED" != "true" ]] && return
    _ARTEMIS_TERM_PENDING_CMD="$1"
    _ARTEMIS_TERM_PENDING_TS="$(date +%s)"
    # OSC 133 B: prompt end / command start
    _artemis_osc133_emit "B"
    # OSC 133 C: command output start
    _artemis_osc133_emit "C"
}

# Called after each command completes, before the next prompt is drawn.
# Captures exit code, pushes to ring buffer, emits OSC 133 D+A markers.
function _artemis_context_precmd() {
    local last_exit=$?  # MUST be first line to capture exit code

    # OSC 133 D: command finished with exit code.
    # Emitted unconditionally (before the enabled check) so that terminals
    # relying on paired A/B/C/D markers never receive an unpaired sequence,
    # even when context capture is disabled.
    _artemis_osc133_emit "D;$last_exit"

    [[ "$_ARTEMIS_TERM_ENABLED" != "true" ]] && return

    # Only record if we have a pending command from preexec
    if [[ -n "$_ARTEMIS_TERM_PENDING_CMD" ]]; then
        _ARTEMIS_TERM_COMMANDS+=("$_ARTEMIS_TERM_PENDING_CMD")
        _ARTEMIS_TERM_EXIT_CODES+=("$last_exit")
        _ARTEMIS_TERM_TIMESTAMPS+=("$_ARTEMIS_TERM_PENDING_TS")

        # Trim ring buffer to max size
        while (( ${#_ARTEMIS_TERM_COMMANDS} > _ARTEMIS_TERM_MAX_COMMANDS )); do
            shift _ARTEMIS_TERM_COMMANDS
            shift _ARTEMIS_TERM_EXIT_CODES
            shift _ARTEMIS_TERM_TIMESTAMPS
        done

        _ARTEMIS_TERM_PENDING_CMD=""
        _ARTEMIS_TERM_PENDING_TS=""
    fi

    # OSC 133 A: prompt start (for the next prompt)
    _artemis_osc133_emit "A"
}

# Hook registration

# Register using standard zsh hook arrays for coexistence with other plugins.
# precmd is prepended so it runs first and captures the real $? from the
# command, before other plugins (powerlevel10k, starship, etc.) overwrite it.
if [[ "$_ARTEMIS_TERM_ENABLED" == "true" ]]; then
    preexec_functions+=(_artemis_context_preexec)
    precmd_functions=(_artemis_context_precmd "${precmd_functions[@]}")
fi
