#!/usr/bin/env zsh

# Enable prompt substitution for RPROMPT
setopt PROMPT_SUBST

# Model and agent info with token count
# Fully formatted output directly from Rust
# Returns ZSH-formatted string ready for use in RPROMPT
function _artemis_prompt_info() {
    local artemis_bin="${_ARTEMIS_BIN:-${ARTEMIS_BIN:-artemis}}"
    
    # Get fully formatted prompt from artemis (single command).
    # Pass session model/provider as CLI flags when set so the rprompt
    # reflects the active session override rather than global config.
    local -a artemis_cmd
    artemis_cmd=("$artemis_bin")
    artemis_cmd+=(zsh rprompt)
    [[ -n "$_ARTEMIS_SESSION_MODEL" ]] && local -x ARTEMIS_SESSION__MODEL_ID="$_ARTEMIS_SESSION_MODEL"
    [[ -n "$_ARTEMIS_SESSION_PROVIDER" ]] && local -x ARTEMIS_SESSION__PROVIDER_ID="$_ARTEMIS_SESSION_PROVIDER"
    [[ -n "$_ARTEMIS_SESSION_REASONING_EFFORT" ]] && local -x ARTEMIS_REASONING__EFFORT="$_ARTEMIS_SESSION_REASONING_EFFORT"
    _ARTEMIS_CONVERSATION_ID=$_ARTEMIS_CONVERSATION_ID _ARTEMIS_ACTIVE_AGENT=$_ARTEMIS_ACTIVE_AGENT "${artemis_cmd[@]}" 2>/dev/null
}

# Right prompt: agent and model with token count (uses single artemis prompt command)
# Set RPROMPT if empty, otherwise append to existing value
if [[ -z "$_ARTEMIS_THEME_LOADED" ]]; then
    RPROMPT='$(_artemis_prompt_info)'"${RPROMPT:+ ${RPROMPT}}"
fi
