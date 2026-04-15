#!/usr/bin/env zsh

# Authentication action handlers

# Action handler: Login to provider
function _artemis_action_login() {
    local input_text="$1"
    echo
    local selected
    # Pass input_text as query parameter for fuzzy search
    selected=$(_artemis_select_provider "" "" "" "$input_text")
    if [[ -n "$selected" ]]; then
        # Extract the second field (provider ID)
        # Use multi-space delimiter to handle display names containing spaces
        local provider=$(echo "$selected" | awk -F '  +' '{print $2}')
        _artemis_exec_interactive provider login "$provider"
    fi
}

# Action handler: Logout from provider
function _artemis_action_logout() {
    local input_text="$1"
    echo
    local selected
    # Pass input_text as query parameter for fuzzy search
    selected=$(_artemis_select_provider "\[yes\]" "" "" "$input_text")
    if [[ -n "$selected" ]]; then
        # Extract the second field (provider ID)
        # Use multi-space delimiter to handle display names containing spaces
        local provider=$(echo "$selected" | awk -F '  +' '{print $2}')
        _artemis_exec provider logout "$provider"
    fi
}
