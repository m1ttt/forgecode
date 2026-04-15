# !! Contents within this block are managed by 'artemis zsh setup' !!
# !! Do not edit manually - changes will be overwritten !!

# Add required zsh plugins if not already present
if [[ ! " ${plugins[@]} " =~ " zsh-autosuggestions " ]]; then
    plugins+=(zsh-autosuggestions)
fi
if [[ ! " ${plugins[@]} " =~ " zsh-syntax-highlighting " ]]; then
    plugins+=(zsh-syntax-highlighting)
fi

# Load artemis shell plugin (commands, completions, keybindings) if not already loaded
if [[ -z "$_ARTEMIS_PLUGIN_LOADED" ]]; then
    eval "$(artemis zsh plugin)"
fi

# Load artemis shell theme (prompt with AI context) if not already loaded
if [[ -z "$_ARTEMIS_THEME_LOADED" ]]; then
    eval "$(artemis zsh theme)"
fi
