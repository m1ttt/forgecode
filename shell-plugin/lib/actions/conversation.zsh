#!/usr/bin/env zsh

# Conversation management action handlers
# 
# Features:
# - :conversation          - List and switch conversations (with fzf)
# - :conversation <id>     - Switch to specific conversation by ID
# - :conversation -        - Toggle between current and previous conversation (like cd -)
# - :clone                 - Clone current or selected conversation
# - :clone <id>            - Clone specific conversation by ID
# - :copy                  - Copy last assistant message to OS clipboard as raw markdown
# - :rename <name>         - Rename the current conversation
# - :conversation-rename   - Rename a conversation (interactive picker)
# - :conversation-rename <id> <name> - Rename specific conversation by ID
#
# Helper Functions:
# - _artemis_switch_conversation <id>  - Switch to a conversation and track previous
# - _artemis_clear_conversation        - Clear conversation and save as previous

# Helper function to switch to a conversation and track previous (like cd -)
function _artemis_switch_conversation() {
    local new_conversation_id="$1"
    
    # Only update previous if we're switching to a different conversation
    if [[ -n "$_ARTEMIS_CONVERSATION_ID" && "$_ARTEMIS_CONVERSATION_ID" != "$new_conversation_id" ]]; then
        # Save current as previous
        _ARTEMIS_PREVIOUS_CONVERSATION_ID="$_ARTEMIS_CONVERSATION_ID"
    fi
    
    # Set the new conversation as active
    _ARTEMIS_CONVERSATION_ID="$new_conversation_id"
}

# Helper function to reset/clear conversation and track previous (like cd -)
function _artemis_clear_conversation() {
    # Save current as previous before clearing
    if [[ -n "$_ARTEMIS_CONVERSATION_ID" ]]; then
        _ARTEMIS_PREVIOUS_CONVERSATION_ID="$_ARTEMIS_CONVERSATION_ID"
    fi
    
    # Clear the current conversation
    _ARTEMIS_CONVERSATION_ID=""
}

# Action handler: List/switch conversations
function _artemis_action_conversation() {
    local input_text="$1"
    
    echo
    
    # Handle toggling to previous conversation (like cd -)
    if [[ "$input_text" == "-" ]]; then
        # Check if there's a previous conversation
        if [[ -z "$_ARTEMIS_PREVIOUS_CONVERSATION_ID" ]]; then
            # No previous conversation tracked, show conversation list like :conversation
            input_text=""
            # Fall through to the conversation list logic below
        else
            # Swap current and previous
            local temp="$_ARTEMIS_CONVERSATION_ID"
            _ARTEMIS_CONVERSATION_ID="$_ARTEMIS_PREVIOUS_CONVERSATION_ID"
            _ARTEMIS_PREVIOUS_CONVERSATION_ID="$temp"
            
            # Show conversation content
            echo
            _artemis_exec conversation show "$_ARTEMIS_CONVERSATION_ID"
            
            # Show conversation info
            _artemis_exec conversation info "$_ARTEMIS_CONVERSATION_ID"
            
            # Print log about conversation switching
            _artemis_log success "Switched to conversation \033[1m${_ARTEMIS_CONVERSATION_ID}\033[0m"
            
            return 0
        fi
    fi
    
    # If an ID is provided directly, use it
    if [[ -n "$input_text" ]]; then
        local conversation_id="$input_text"
        
        # Switch to conversation and track in history
        _artemis_switch_conversation "$conversation_id"
        
        # Show conversation content
        echo
        _artemis_exec conversation show "$conversation_id"
        
        # Show conversation info
        _artemis_exec conversation info "$conversation_id"
        
        # Print log about conversation switching
        _artemis_log success "Switched to conversation \033[1m${conversation_id}\033[0m"
        
        return 0
    fi
    
    # Get conversations list
    local conversations_output
    conversations_output=$($_ARTEMIS_BIN conversation list --porcelain 2>/dev/null)
    
    if [[ -n "$conversations_output" ]]; then
        # Get current conversation ID if set
        local current_id="$_ARTEMIS_CONVERSATION_ID"
        
        # Create prompt with current conversation
        local prompt_text="Conversation ❯ "
        local fzf_args=(
            --prompt="$prompt_text"
            --delimiter="$_ARTEMIS_DELIMITER"
            --with-nth="2,3"
            --preview="CLICOLOR_FORCE=1 $_ARTEMIS_BIN conversation info {1}; echo; CLICOLOR_FORCE=1 $_ARTEMIS_BIN conversation show {1}"
            $_ARTEMIS_PREVIEW_WINDOW
        )

        # If there's a current conversation, position cursor on it
        if [[ -n "$current_id" ]]; then
            # For conversations, compare against the first field (conversation_id)
            local index=$(_artemis_find_index "$conversations_output" "$current_id" 1)
            fzf_args+=(--bind="start:pos($index)")
        fi

        local selected_conversation
        # Use fzf with preview showing the last message from the conversation
        selected_conversation=$(echo "$conversations_output" | _artemis_fzf --header-lines=1 "${fzf_args[@]}")
        
        if [[ -n "$selected_conversation" ]]; then
            # Extract the first field (UUID) - everything before the first multi-space delimiter
            local conversation_id=$(echo "$selected_conversation" | sed -E 's/  .*//' | tr -d '\n')
            
            # Switch to conversation and track in history
            _artemis_switch_conversation "$conversation_id"
            
            # Show conversation content
            echo
            _artemis_exec conversation show "$conversation_id"
            
            # Show conversation info
            _artemis_exec conversation info "$conversation_id"
            
            # Print log about conversation switching
            _artemis_log success "Switched to conversation \033[1m${conversation_id}\033[0m"
            
        fi
    else
        _artemis_log error "No conversations found"
    fi
}

# Action handler: Clone conversation
function _artemis_action_clone() {
    local input_text="$1"
    local clone_target="$input_text"
    
    echo
    
    # Handle explicit clone target if provided
    if [[ -n "$clone_target" ]]; then
        _artemis_clone_and_switch "$clone_target"
        return 0
    fi
    
    # Get conversations list for fzf selection
    local conversations_output
    conversations_output=$($_ARTEMIS_BIN conversation list --porcelain 2>/dev/null)
    
    if [[ -z "$conversations_output" ]]; then
        _artemis_log error "No conversations found"
        return 0
    fi
    
    # Get current conversation ID if set
    local current_id="$_ARTEMIS_CONVERSATION_ID"
    
    # Create fzf interface similar to :conversation
    local prompt_text="Clone Conversation ❯ "
    local fzf_args=(
        --prompt="$prompt_text"
        --delimiter="$_ARTEMIS_DELIMITER"
        --with-nth="2,3"
        --preview="CLICOLOR_FORCE=1 $_ARTEMIS_BIN conversation info {1}; echo; CLICOLOR_FORCE=1 $_ARTEMIS_BIN conversation show {1}"
        $_ARTEMIS_PREVIEW_WINDOW
    )

    # Position cursor on current conversation if available
    if [[ -n "$current_id" ]]; then
        local index=$(_artemis_find_index "$conversations_output" "$current_id")
        fzf_args+=(--bind="start:pos($index)")
    fi

    local selected_conversation
    selected_conversation=$(echo "$conversations_output" | _artemis_fzf --header-lines=1 "${fzf_args[@]}")
    
    if [[ -n "$selected_conversation" ]]; then
        # Extract conversation ID
        local conversation_id=$(echo "$selected_conversation" | sed -E 's/  .*//' | tr -d '\n')
        _artemis_clone_and_switch "$conversation_id"
    fi
}

# Action handler: Copy last assistant message to OS clipboard as raw markdown
# Usage: :copy
function _artemis_action_copy() {
    echo

    if [[ -z "$_ARTEMIS_CONVERSATION_ID" ]]; then
        _artemis_log error "No active conversation. Start a conversation first or use :conversation to see existing ones"
        return 0
    fi

    # Fetch raw markdown from the last assistant message
    local content
    content=$($_ARTEMIS_BIN conversation show --md "$_ARTEMIS_CONVERSATION_ID" 2>/dev/null)

    if [[ -z "$content" ]]; then
        _artemis_log error "No assistant message found in the current conversation"
        return 0
    fi

    # Copy to clipboard (pbcopy on macOS, xclip/xsel on Linux)
    if command -v pbcopy &>/dev/null; then
        echo -n "$content" | pbcopy
    elif command -v xclip &>/dev/null; then
        echo -n "$content" | xclip -selection clipboard
    elif command -v xsel &>/dev/null; then
        echo -n "$content" | xsel --clipboard --input
    else
        _artemis_log error "No clipboard utility found (pbcopy, xclip, or xsel required)"
        return 0
    fi

    # Count lines and bytes for the confirmation message
    local line_count byte_count
    line_count=$(echo "$content" | wc -l | tr -d ' ')
    byte_count=$(echo -n "$content" | wc -c | tr -d ' ')

    _artemis_log success "Copied to clipboard \033[90m[${line_count} lines, ${byte_count} bytes]\033[0m"
}

# Action handler: Rename current conversation
# Usage: :rename <name>
function _artemis_action_rename() {
    local input_text="$1"

    echo

    if [[ -z "$_ARTEMIS_CONVERSATION_ID" ]]; then
        _artemis_log error "No active conversation. Start a conversation first or use :conversation to select one"
        return 0
    fi

    if [[ -z "$input_text" ]]; then
        _artemis_log error "Usage: :rename <name>"
        return 0
    fi

    _artemis_exec conversation rename "$_ARTEMIS_CONVERSATION_ID" $input_text
}

# Action handler: Rename a conversation (interactive picker or by ID)
# Usage: :conversation-rename [<id> <name>]
function _artemis_action_conversation_rename() {
    local input_text="$1"

    echo

    # If input looks like "<id> <name>", split and rename directly
    if [[ -n "$input_text" ]]; then
        local conversation_id="${input_text%% *}"
        local new_name="${input_text#* }"

        if [[ "$conversation_id" == "$new_name" ]]; then
            # Only one arg provided — not enough
            _artemis_log error "Usage: :conversation-rename <id> <name>"
            return 0
        fi

        _artemis_exec conversation rename "$conversation_id" $new_name
        return 0
    fi

    # No args — show interactive picker
    local conversations_output
    conversations_output=$($_ARTEMIS_BIN conversation list --porcelain 2>/dev/null)

    if [[ -z "$conversations_output" ]]; then
        _artemis_log error "No conversations found"
        return 0
    fi

    local current_id="$_ARTEMIS_CONVERSATION_ID"

    local prompt_text="Rename Conversation ❯ "
    local fzf_args=(
        --prompt="$prompt_text"
        --delimiter="$_ARTEMIS_DELIMITER"
        --with-nth="2,3"
        --preview="CLICOLOR_FORCE=1 $_ARTEMIS_BIN conversation info {1}; echo; CLICOLOR_FORCE=1 $_ARTEMIS_BIN conversation show {1}"
        $_ARTEMIS_PREVIEW_WINDOW
    )

    if [[ -n "$current_id" ]]; then
        local index=$(_artemis_find_index "$conversations_output" "$current_id" 1)
        fzf_args+=(--bind="start:pos($index)")
    fi

    local selected_conversation
    selected_conversation=$(echo "$conversations_output" | _artemis_fzf --header-lines=1 "${fzf_args[@]}")

    if [[ -n "$selected_conversation" ]]; then
        local conversation_id=$(echo "$selected_conversation" | sed -E 's/  .*//' | tr -d '\n')

        # Prompt for new name
        echo -n "Enter new name: "
        read -r new_name </dev/tty

        if [[ -n "$new_name" ]]; then
            _artemis_exec conversation rename "$conversation_id" $new_name
        else
            _artemis_log error "No name provided, rename cancelled"
        fi
    fi
}

# Helper function to clone and switch to conversation
function _artemis_clone_and_switch() {
    local clone_target="$1"
    
    # Store original conversation ID to check if we're cloning current conversation
    local original_conversation_id="$_ARTEMIS_CONVERSATION_ID"
    
    # Execute clone command
    _artemis_log info "Cloning conversation \033[1m${clone_target}\033[0m"
    local clone_output
    clone_output=$($_ARTEMIS_BIN conversation clone "$clone_target" 2>&1)
    local clone_exit_code=$?
    
    if [[ $clone_exit_code -eq 0 ]]; then
        # Extract new conversation ID from output
        local new_id=$(echo "$clone_output" | grep -oE '[a-f0-9-]{36}' | tail -1)
        
        if [[ -n "$new_id" ]]; then
            # Switch to cloned conversation and track previous
            _artemis_switch_conversation "$new_id"
            
            _artemis_log success "└─ Switched to conversation \033[1m${new_id}\033[0m"
            
            # Show content and info only if cloning a different conversation (not current one)
            if [[ "$clone_target" != "$original_conversation_id" ]]; then
                echo
                _artemis_exec conversation show "$new_id"
                
                # Show new conversation info
                echo
                _artemis_exec conversation info "$new_id"
            fi
        else
            _artemis_log error "Failed to extract new conversation ID from clone output"
        fi
    else
        _artemis_log error "Failed to clone conversation: $clone_output"
    fi
}
