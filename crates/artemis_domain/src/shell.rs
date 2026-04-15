/// Output from a command execution
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
}

impl CommandOutput {
    pub fn success(&self) -> bool {
        self.exit_code.is_none_or(|code| code >= 0)
    }
}

/// Result returned when a shell job is started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandStart {
    pub job_id: u64,
    pub command: String,
}

/// Snapshot of a shell job state.
#[derive(Debug, Clone)]
pub struct CommandJobSnapshot {
    pub job_id: u64,
    pub output: CommandOutput,
    pub running: bool,
}

/// Result returned when requesting job termination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandKill {
    pub job_id: u64,
    pub killed: bool,
}
