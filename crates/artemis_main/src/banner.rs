use std::{fmt, io};

use colored::Colorize;
use artemis_tracker::VERSION;

const BANNER: &str = include_str!("banner");

/// Checks the primary environment variable first, falling back to a legacy
/// name for backward compatibility.
fn env_var_or_legacy(name: &str, legacy: &str) -> Option<String> {
    std::env::var(name).ok().or_else(|| std::env::var(legacy).ok())
}

/// Renders messages into a styled box with border characters.
struct DisplayBox {
    messages: Vec<String>,
}

impl DisplayBox {
    /// Creates a new Box with the given messages.
    fn new(messages: Vec<String>) -> Self {
        Self { messages }
    }
}

impl fmt::Display for DisplayBox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let visible_len = |s: &str| console::measure_text_width(s);
        let width: usize = self
            .messages
            .iter()
            .map(|s| visible_len(s))
            .max()
            .unwrap_or(0)
            + 4;
        let top = format!("┌{}┐", "─".repeat(width.saturating_sub(2)));
        let bottom = format!("└{}┘", "─".repeat(width.saturating_sub(2)));
        let fmt_line = |s: &str| {
            let padding = width.saturating_sub(4).saturating_sub(visible_len(s));
            format!("│ {}{} │", s, " ".repeat(padding))
        };

        writeln!(f, "{}", top)?;
        for msg in &self.messages {
            writeln!(f, "{}", fmt_line(msg))?;
        }
        write!(f, "{}", bottom)
    }
}

/// Displays the banner with version and command tips.
///
/// # Arguments
///
/// * `cli_mode` - If true, shows CLI-relevant commands. Both interactive and
///   CLI modes use `:` as the canonical command prefix.
///
/// # Environment Variables
///
/// * `ARTEMIS_BANNER` - Optional custom banner text to display instead of the
///   default (falls back to `FORGE_BANNER` for backward compatibility)
pub fn display(cli_mode: bool) -> io::Result<()> {
    // Check for custom banner via environment variable
    let mut banner = env_var_or_legacy("ARTEMIS_BANNER", "FORGE_BANNER")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| BANNER.to_string());

    // Always show version
    let version_label = ("Version:", VERSION);

    // Build tips based on mode
    let tips: Vec<(&str, &str)> = if cli_mode {
        // CLI mode: only show relevant commands
        vec![
            ("Start chatting:", "type your message and press Enter"),
            ("New conversation:", ":new"),
            ("Switch model:", ":model"),
            ("Switch provider:", ":provider"),
            ("Switch agent:", ":<agent_name> e.g. :artemis or :muse"),
        ]
    } else {
        // Interactive mode: show all commands
        vec![
            ("Start chatting:", "type your message and press Enter"),
            ("New conversation:", ":new"),
            ("Get started:", ":info, :usage, :help, :conversation"),
            ("Switch model:", ":model"),
            ("Switch agent:", ":artemis or :muse or :agent"),
            ("Multiline:", "Alt+Enter for new line"),
            ("Quit:", ":exit or <CTRL+D>"),
        ]
    };

    // Build labels array with version and tips
    let labels: Vec<(&str, &str)> = std::iter::once(version_label).chain(tips).collect();

    // Calculate the width of the longest label key for alignment
    let max_width = labels.iter().map(|(key, _)| key.len()).max().unwrap_or(0);

    // Add all lines with right-aligned label keys and their values
    for (key, value) in &labels {
        banner.push_str(
            format!(
                "\n{}{}",
                format!("{key:>max_width$} ").dimmed(),
                value.cyan()
            )
            .as_str(),
        );
    }

    println!("{banner}\n");

    // Encourage zsh integration after the banner
    if !cli_mode {
        display_zsh_encouragement();
    }

    Ok(())
}

/// Encourages users to use the zsh plugin for a better experience.
fn display_zsh_encouragement() {
    let tip = DisplayBox::new(vec![
        format!(
            "{} {}",
            "TIP:".bold().yellow(),
            "For the best experience, use our zsh plugin!".bold()
        ),
        format!(
            "{} {} {}",
            "·".dimmed(),
            "Set up artemis via our zsh plugin:".dimmed(),
            "artemis zsh setup".bold().green(),
        ),
        format!(
            "{} {} {}",
            "·".dimmed(),
            "Learn more:".dimmed(),
            "https://artemis.dev/docs/zsh-support".cyan()
        ),
    ]);
    println!("{}", tip);
}
