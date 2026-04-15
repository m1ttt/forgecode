use std::borrow::Cow;
use std::fmt::Write;
use std::path::PathBuf;

use convert_case::{Case, Casing};
use derive_setters::Setters;
use artemis_api::{AgentId, ModelId, Usage};
use nu_ansi_term::{Color, Style};
use reedline::{Prompt, PromptHistorySearchStatus};

use crate::display_constants::markers;
use crate::state::ActiveShellJob;
use crate::utils::humanize_number;

// Characters for the chat-box frame
const CORNER_TL: &str = "\u{256d}"; // ╭
const CORNER_TR: &str = "\u{256e}"; // ╮
const HORIZONTAL: &str = "\u{2500}"; // ─
const VERTICAL: &str = "\u{2502}"; // │

// Input cursor symbol
const INPUT_CURSOR: &str = "\u{276f}"; // ❯

// Separator between header segments
const SEPARATOR: &str = " \u{b7} "; // ·

/// Chat-style prompt for the Artemis agent interface.
///
/// Renders a framed input area with a contextual header bar showing the
/// active agent, working directory, git branch, and model info. The user
/// types inside a visually distinct box, making the input area intuitive
/// and easy to identify.
#[derive(Clone, Setters)]
#[setters(strip_option, borrow_self)]
pub struct ForgePrompt {
    pub cwd: PathBuf,
    pub usage: Option<Usage>,
    pub agent_id: AgentId,
    pub model: Option<ModelId>,
    pub git_branch: Option<String>,
    pub active_shell_job: Option<ActiveShellJob>,
}

impl ForgePrompt {
    /// Creates a new `ForgePrompt`, resolving the git branch once at
    /// construction time.
    pub fn new(cwd: PathBuf, agent_id: AgentId) -> Self {
        let git_branch = get_git_branch();
        Self { cwd, usage: None, agent_id, model: None, git_branch, active_shell_job: None }
    }

    pub fn refresh(&mut self) -> &mut Self {
        let git_branch = get_git_branch();
        self.git_branch = git_branch;
        self
    }

    /// Builds the header line content (what goes inside the top border).
    /// Layout: `AGENT · dir · branch · tokens · cost · model`
    fn header_content(&self) -> String {
        let mut segments = Vec::new();

        // Agent name
        let agent_name = self.agent_id.as_str().to_case(Case::UpperSnake);
        segments.push(
            Style::new()
                .bold()
                .fg(Color::Cyan)
                .paint(agent_name)
                .to_string(),
        );

        // Working directory
        let current_dir = self
            .cwd
            .file_name()
            .and_then(|name| name.to_str())
            .map(String::from)
            .unwrap_or_else(|| markers::EMPTY.to_string());
        segments.push(
            Style::new()
                .fg(Color::LightGray)
                .paint(current_dir)
                .to_string(),
        );

        // Git branch (only when present and different from directory name)
        if let Some(branch) = self.git_branch.as_deref() {
            if branch != self.cwd.file_name().and_then(|n| n.to_str()).unwrap_or("") {
                segments.push(
                    Style::new()
                        .fg(Color::LightGreen)
                        .paint(branch)
                        .to_string(),
                );
            }
        }

        // Token count and cost (only when active)
        let total_tokens = self.usage.as_ref().map(|u| u.total_tokens);
        let active = total_tokens.map(|t| *t > 0).unwrap_or(false);

        if let Some(tokens) = total_tokens {
            if active {
                let prefix = match tokens {
                    artemis_api::TokenCount::Actual(_) => "",
                    artemis_api::TokenCount::Approx(_) => "~",
                };
                let count_str = format!("{}{} tok", prefix, humanize_number(*tokens));
                segments.push(
                    Style::new()
                        .fg(Color::LightGray)
                        .paint(&count_str)
                        .to_string(),
                );
            }
        }

        if let Some(cost) = self.usage.as_ref().and_then(|u| u.cost) {
            if active {
                segments.push(
                    Style::new()
                        .fg(Color::Green)
                        .paint(format!("${cost:.2}"))
                        .to_string(),
                );
            }
        }

        // Model name
        if let Some(model) = self.model.as_ref() {
            let model_str = model.to_string();
            let short_model = model_str.split('/').next_back().unwrap_or(model.as_str());
            segments.push(
                Style::new()
                    .fg(Color::LightMagenta)
                    .paint(short_model)
                    .to_string(),
            );
        }

        segments.join(
            Style::new()
                .fg(Color::DarkGray)
                .paint(SEPARATOR)
                .to_string()
                .as_str(),
        )
    }
}

impl Prompt for ForgePrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        // Chat-box layout:
        //
        //   ╭─ ARTEMIS · my-project · main · claude-3 ───╮
        //   ❯
        //
        // The top border contains contextual info. The input cursor on the
        // next line invites the user to type.

        let header = self.header_content();

        let mut result = String::with_capacity(256);

        // Top border: ╭─ header ───╮
        write!(result, "{}", Style::new().fg(Color::DarkGray).paint(CORNER_TL)).unwrap();
        write!(result, "{}", Style::new().fg(Color::DarkGray).paint(HORIZONTAL)).unwrap();
        write!(result, " {header} ").unwrap();
        write!(
            result,
            "{}",
            Style::new()
                .fg(Color::DarkGray)
                .paint(format!("{HORIZONTAL}{HORIZONTAL}{HORIZONTAL}"))
        )
        .unwrap();
        write!(result, "{}", Style::new().fg(Color::DarkGray).paint(CORNER_TR)).unwrap();

        // Input line: cursor ready for typing
        write!(
            result,
            "\n{} ",
            Style::new().bold().fg(Color::Green).paint(INPUT_CURSOR)
        )
        .unwrap();

        Cow::Owned(result)
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        let Some(job) = self.active_shell_job.as_ref().filter(|job| job.running) else {
            return Cow::Borrowed("");
        };

        let mut segments = vec![
            Style::new()
                .fg(Color::Yellow)
                .paint(format!("job #{} running", job.job_id))
                .to_string(),
            Style::new()
                .fg(Color::LightYellow)
                .paint(truncate_for_prompt(&job.command, 28))
                .to_string(),
        ];

        if let Some(preview) = job.preview.as_deref().filter(|preview| !preview.trim().is_empty()) {
            segments.push(
                Style::new()
                    .fg(Color::DarkGray)
                    .paint(truncate_for_prompt(preview.trim(), 28))
                    .to_string(),
            );
        }

        Cow::Owned(
            segments.join(
                Style::new()
                    .fg(Color::DarkGray)
                    .paint(SEPARATOR)
                    .to_string()
                    .as_str(),
            ),
        )
    }

    fn render_prompt_indicator(&self, _prompt_mode: reedline::PromptEditMode) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        // Continuation line for multiline input: vertical bar aligned with
        // the input cursor
        let indent = INPUT_CURSOR.len();
        let padding = " ".repeat(indent);
        Cow::Owned(format!(
            "{}{} ",
            padding,
            Style::new().fg(Color::DarkGray).paint(VERTICAL)
        ))
    }

    fn render_prompt_history_search_indicator(
        &self,
        history_search: reedline::PromptHistorySearch,
    ) -> Cow<'_, str> {
        let prefix = match history_search.status {
            PromptHistorySearchStatus::Passing => "",
            PromptHistorySearchStatus::Failing => "failing ",
        };

        let mut result = String::with_capacity(32);

        if history_search.term.is_empty() {
            write!(result, "({prefix}reverse-search) ").unwrap();
        } else {
            write!(
                result,
                "({}reverse-search: {}) ",
                prefix, history_search.term
            )
            .unwrap();
        }

        Cow::Owned(Style::new().fg(Color::White).paint(&result).to_string())
    }
}

/// Gets the current git branch name if available.
fn get_git_branch() -> Option<String> {
    let repo = gix::discover(".").ok()?;
    let head = repo.head().ok()?;
    head.referent_name().map(|r| r.shorten().to_string())
}

fn truncate_for_prompt(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= max_chars {
        return trimmed.to_string();
    }

    let keep = max_chars.saturating_sub(3);
    chars.into_iter().take(keep).collect::<String>() + "..."
}

#[cfg(test)]
mod tests {
    use nu_ansi_term::Style;
    use pretty_assertions::assert_eq;

    use super::*;

    impl Default for ForgePrompt {
        fn default() -> Self {
            ForgePrompt {
                cwd: PathBuf::from("."),
                usage: None,
                agent_id: AgentId::default(),
                model: None,
                git_branch: None,
                active_shell_job: None,
            }
        }
    }

    #[test]
    fn test_render_prompt_left_has_box_frame() {
        let prompt = ForgePrompt::default();
        let actual = prompt.render_prompt_left();

        // Top border corners present
        assert!(actual.contains(CORNER_TL));
        assert!(actual.contains(CORNER_TR));
        // Input cursor present
        assert!(actual.contains(INPUT_CURSOR));
    }

    #[test]
    fn test_render_prompt_left_shows_agent_name() {
        let prompt = ForgePrompt::default();
        let actual = prompt.render_prompt_left();

        // Agent name (ARTEMIS by default) appears in the header
        assert!(actual.contains("ARTEMIS"));
    }

    #[test]
    fn test_render_prompt_left_with_branch() {
        let mut prompt = ForgePrompt::default();
        prompt.git_branch = Some("main".to_string());
        let actual = prompt.render_prompt_left();

        assert!(actual.contains("main"));
    }

    #[test]
    fn test_render_prompt_right_is_empty() {
        let mut prompt = ForgePrompt::default();
        let _ = prompt.model(ModelId::new("gpt-4"));

        let actual = prompt.render_prompt_right();
        assert_eq!(actual.as_ref(), "");
    }

    #[test]
    fn test_render_prompt_right_active_with_tokens() {
        let usage = Usage {
            prompt_tokens: artemis_api::TokenCount::Actual(10),
            completion_tokens: artemis_api::TokenCount::Actual(20),
            total_tokens: artemis_api::TokenCount::Approx(30),
            ..Default::default()
        };
        let mut prompt = ForgePrompt::default();
        let _ = prompt.usage(usage);

        let actual = prompt.render_prompt_right();
        assert_eq!(actual.as_ref(), "");
    }

    #[test]
    fn test_render_prompt_right_with_running_job() {
        let mut prompt = ForgePrompt::default();
        let _ = prompt.active_shell_job(
            ActiveShellJob::default()
                .job_id(42_u64)
                .command("cargo test --package artemis_main")
                .running(true)
                .preview("Compiling artemis_main v0.1.0"),
        );

        let actual = prompt.render_prompt_right();

        assert!(actual.contains("job #42 running"));
        assert!(actual.contains("cargo test --package"));
        assert!(actual.contains("Compiling artemis_main"));
    }

    #[test]
    fn test_render_prompt_multiline_indicator() {
        let prompt = ForgePrompt::default();
        let actual = prompt.render_prompt_multiline_indicator();
        // Should contain the vertical bar for continuation
        assert!(actual.contains(VERTICAL));
    }

    #[test]
    fn test_render_prompt_history_search_indicator_passing() {
        let prompt = ForgePrompt::default();
        let history_search = reedline::PromptHistorySearch {
            status: PromptHistorySearchStatus::Passing,
            term: "test".to_string(),
        };
        let actual = prompt.render_prompt_history_search_indicator(history_search);
        let expected = Style::new()
            .fg(Color::White)
            .paint("(reverse-search: test) ")
            .to_string();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_render_prompt_history_search_indicator_failing() {
        let prompt = ForgePrompt::default();
        let history_search = reedline::PromptHistorySearch {
            status: PromptHistorySearchStatus::Failing,
            term: "test".to_string(),
        };
        let actual = prompt.render_prompt_history_search_indicator(history_search);
        let expected = Style::new()
            .fg(Color::White)
            .paint("(failing reverse-search: test) ")
            .to_string();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_render_prompt_history_search_indicator_empty_term() {
        let prompt = ForgePrompt::default();
        let history_search = reedline::PromptHistorySearch {
            status: PromptHistorySearchStatus::Passing,
            term: "".to_string(),
        };
        let actual = prompt.render_prompt_history_search_indicator(history_search);
        let expected = Style::new()
            .fg(Color::White)
            .paint("(reverse-search) ")
            .to_string();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_header_content_shows_model() {
        let mut prompt = ForgePrompt::default();
        let _ = prompt.model(ModelId::new("anthropic/claude-3"));

        let actual = prompt.header_content();
        assert!(actual.contains("claude-3"));
        assert!(!actual.contains("anthropic/claude-3"));
    }

    #[test]
    fn test_header_content_with_tokens_and_cost() {
        let usage = Usage {
            total_tokens: artemis_api::TokenCount::Actual(1500),
            cost: Some(0.01),
            ..Default::default()
        };
        let mut prompt = ForgePrompt::default();
        let _ = prompt.usage(usage);

        let actual = prompt.header_content();
        assert!(actual.contains("1.5k"));
        assert!(actual.contains("0.01"));
    }

    #[test]
    fn test_render_prompt_right_strips_provider_prefix() {
        // Right prompt is now empty — model info is in the header bar instead
        let usage = Usage {
            prompt_tokens: artemis_api::TokenCount::Actual(10),
            completion_tokens: artemis_api::TokenCount::Actual(20),
            total_tokens: artemis_api::TokenCount::Actual(30),
            ..Default::default()
        };
        let mut prompt = ForgePrompt::default();
        let _ = prompt.usage(usage);
        let _ = prompt.model(ModelId::new("anthropic/claude-3"));

        let actual = prompt.render_prompt_right();
        assert_eq!(actual.as_ref(), "");
    }

    #[test]
    fn test_render_prompt_right_with_cost() {
        // Right prompt is now empty — cost info is in the header bar instead
        let usage = Usage {
            total_tokens: artemis_api::TokenCount::Actual(1500),
            cost: Some(0.01),
            ..Default::default()
        };
        let mut prompt = ForgePrompt::default();
        let _ = prompt.usage(usage);

        let actual = prompt.render_prompt_right();
        assert_eq!(actual.as_ref(), "");
    }
}
