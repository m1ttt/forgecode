use std::path::PathBuf;

use derive_setters::Setters;
use artemis_api::{ConversationId, Environment};

/// Shell job status rendered in the interactive prompt.
#[derive(Debug, Default, Clone, PartialEq, Eq, Setters)]
#[setters(strip_option, into)]
pub struct ActiveShellJob {
    pub job_id: u64,
    pub command: String,
    pub running: bool,
    pub preview: Option<String>,
}

//TODO: UIState and ForgePrompt seem like the same thing and can be merged
/// State information for the UI
#[derive(Debug, Default, Clone, Setters)]
#[setters(strip_option)]
pub struct UIState {
    pub cwd: PathBuf,
    pub conversation_id: Option<ConversationId>,
    pub active_shell_job: Option<ActiveShellJob>,
}

impl UIState {
    pub fn new(env: Environment) -> Self {
        Self {
            cwd: env.cwd,
            conversation_id: Default::default(),
            active_shell_job: Default::default(),
        }
    }
}
