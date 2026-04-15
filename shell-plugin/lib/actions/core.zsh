#!/usr/bin/env zsh

# Core action handlers for basic artemis operations

# Action handler: Start a new conversation
function _artemis_action_new() {
    local input_text="$1"
    
    # Clear conversation and save as previous (like cd -)
    _artemis_clear_conversation
    _ARTEMIS_ACTIVE_AGENT="artemis"
    
    echo
    
    # If input_text is provided, send it to the new conversation
    if [[ -n "$input_text" ]]; then
        # Generate new conversation ID and switch to it
        local new_id=$($_ARTEMIS_BIN conversation new)
        _artemis_switch_conversation "$new_id"
        
        # Execute the artemis command with the input text
        _artemis_exec_interactive -p "$input_text" --cid "$_ARTEMIS_CONVERSATION_ID"
        
        # Start background sync job if enabled and not already running
        _artemis_start_background_sync
        # Start background update check
        _artemis_start_background_update
    else
        # Only show banner if no input text (starting fresh conversation)
        _artemis_exec banner
    fi
}

# Action handler: Show session info
function _artemis_action_info() {
    echo
    if [[ -n "$_ARTEMIS_CONVERSATION_ID" ]]; then
        _artemis_exec info --cid "$_ARTEMIS_CONVERSATION_ID"
    else
        _artemis_exec info
    fi
}

# Action handler: Dump conversation
function _artemis_action_dump() {
    local input_text="$1"
    if [[ "$input_text" == "html" ]]; then
        _artemis_handle_conversation_command "dump" "--html"
    else
        _artemis_handle_conversation_command "dump"
    fi
}

# Action handler: Compact conversation
function _artemis_action_compact() {
    _artemis_handle_conversation_command "compact"
}

# Action handler: Retry last message
function _artemis_action_retry() {
    _artemis_handle_conversation_command "retry"
}

# Helper function to handle conversation commands that require an active conversation
function _artemis_handle_conversation_command() {
    local subcommand="$1"
    shift  # Remove first argument, remaining args become extra parameters
    
    echo
    
    # Check if ARTEMIS_CONVERSATION_ID is set
    if [[ -z "$_ARTEMIS_CONVERSATION_ID" ]]; then
        _artemis_log error "No active conversation. Start a conversation first or use :conversation to see existing ones"
        return 0
    fi
    
    # Execute the conversation command with conversation ID and any extra arguments
    _artemis_exec conversation "$subcommand" "$_ARTEMIS_CONVERSATION_ID" "$@"
}
