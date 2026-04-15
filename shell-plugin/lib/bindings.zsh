#!/usr/bin/env zsh

# Key bindings and widget registration for artemis plugin

# Register ZLE widgets
zle -N artemis-accept-line
zle -N artemis-completion

# Custom bracketed-paste handler that wraps dropped file paths in @[] syntax
# and fixes syntax highlighting after paste.
#
# Path detection and wrapping is delegated to `artemis zsh format` (Rust) so
# that all parsing logic lives in one well-tested place.
function artemis-bracketed-paste() {
    # Call the built-in bracketed-paste widget first
    zle .$WIDGET "$@"
    
    # Only auto-wrap when the line is a artemis command (starts with ':').
    # This avoids mangling paths pasted into normal shell commands like
    # 'vim /some/path' or 'cat /some/path'.
    if [[ "$BUFFER" == :* ]]; then
        local formatted=$("$_ARTEMIS_BIN" zsh format --buffer "$BUFFER")
        if [[ -n "$formatted" && "$formatted" != "$BUFFER" ]]; then
            BUFFER="$formatted"
            CURSOR=${#BUFFER}
        fi
    fi
    
    # Explicitly redisplay the buffer to ensure paste content is visible
    # This is critical for large or multiline pastes
    zle redisplay
    
    # Reset the prompt to trigger syntax highlighting refresh
    # The redisplay before reset-prompt ensures the buffer is fully rendered
    zle reset-prompt
}

# Register the bracketed paste widget to fix highlighting on paste
zle -N bracketed-paste artemis-bracketed-paste

# Bind Enter to our custom accept-line that transforms :commands
bindkey '^M' artemis-accept-line
bindkey '^J' artemis-accept-line
# Update the Tab binding to use the new completion widget
bindkey '^I' artemis-completion  # Tab for both @ and :command completion
