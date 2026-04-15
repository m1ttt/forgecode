use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use artemis_domain::{CodebaseQueryResult, ToolCallContext, ToolCatalog, ToolOutput};

use crate::fmt::content::FormatContent;
use crate::operation::{TempContentFiles, ToolOperation};
use crate::services::{Services, ShellService};
use crate::{
    AgentRegistry, ConversationService, EnvironmentInfra, FollowUpService, FsPatchService,
    FsReadService, FsRemoveService, FsSearchService, FsUndoService, FsWriteService,
    ImageReadService, NetFetchService, PlanCreateService, ProviderService, SkillFetchService,
    WorkspaceService,
};

pub struct ToolExecutor<S> {
    services: Arc<S>,
}

fn validate_shell_guardrail(command: &str) -> anyhow::Result<()> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    let lowered = trimmed.to_ascii_lowercase();

    // Catch tool names being incorrectly used as bash commands
    for tool_name in &["shell_poll", "shell_wait", "shell_kill", "shell_exec"] {
        if lowered == *tool_name || lowered.starts_with(&format!("{tool_name} ")) {
            return Err(anyhow!(
                "`{tool_name}` is a tool name, not a shell command. \
                Call it directly as a tool using its parameters (e.g., job_id=<number>), \
                do not pass it to `shell`."
            ));
        }
    }

    if lowered == "wait" || lowered.starts_with("wait ") {
        return Err(anyhow!(
            "Do not use shell with `wait`. Reuse the background job `job_id` with shell_wait."
        ));
    }
    if lowered == "jobs" || lowered.starts_with("jobs ") {
        return Err(anyhow!(
            "Do not use shell with `jobs`. Reuse the background job `job_id` with shell_poll."
        ));
    }
    if lowered.starts_with("sleep ")
        || lowered.contains("&& sleep ")
        || lowered.contains("; sleep ")
    {
        return Err(anyhow!(
            "Do not use shell with `sleep` to monitor a background command. Return control to the user or reuse the existing `job_id` with shell_poll."
        ));
    }
    if lowered.starts_with("ps ") && lowered.contains("grep") {
        return Err(anyhow!(
            "Do not use `ps ... | grep ...` to monitor a background shell command. Reuse the existing `job_id` with shell_poll."
        ));
    }
    if lowered.starts_with("cat /tmp/") || lowered.contains("&& cat /tmp/") {
        return Err(anyhow!(
            "Do not use shell with `cat /tmp/...` to inspect background command output. Use the read tool for files or reuse the existing `job_id` with shell_poll/shell_wait."
        ));
    }

    Ok(())
}

/// Attempts to parse a command of the form `<tool_name> <job_id>` where
/// `tool_name` matches the expected name. Returns the parsed `job_id` if
/// successful, `None` otherwise.
fn parse_tool_as_command(tool_name: &str, lowered_command: &str) -> Option<u64> {
    if lowered_command == tool_name {
        // Called with no argument — treat as job 0 (will produce a clear service error)
        return None;
    }
    let prefix = format!("{tool_name} ");
    if lowered_command.starts_with(&prefix) {
        let rest = lowered_command[prefix.len()..].trim();
        return rest.parse::<u64>().ok();
    }
    None
}


impl<
    S: FsReadService
        + ImageReadService
        + FsWriteService
        + FsSearchService
        + WorkspaceService
        + NetFetchService
        + FsRemoveService
        + FsPatchService
        + FsUndoService
        + ShellService
        + FollowUpService
        + ConversationService
        + EnvironmentInfra<Config = artemis_config::ForgeConfig>
        + PlanCreateService
        + SkillFetchService
        + AgentRegistry
        + ProviderService
        + Services,
> ToolExecutor<S>
{
    pub fn new(services: Arc<S>) -> Self {
        Self { services }
    }

    fn require_prior_read(
        &self,
        context: &ToolCallContext,
        raw_path: &str,
        action: &str,
    ) -> anyhow::Result<()> {
        let target_path = self.normalize_path(raw_path.to_string());
        let has_read = context.with_metrics(|metrics| {
            metrics.files_accessed.contains(&target_path)
                || metrics.files_accessed.contains(raw_path)
        })?;

        if has_read {
            Ok(())
        } else {
            Err(anyhow!(
                "You must read the file with the read tool before attempting to {action}.",
                action = action
            ))
        }
    }

    async fn dump_operation(&self, operation: &ToolOperation) -> anyhow::Result<TempContentFiles> {
        match operation {
            ToolOperation::NetFetch { input: _, output } => {
                let config = self.services.get_config()?;
                let original_length = output.content.len();
                let is_truncated = original_length > config.max_fetch_chars;
                let mut files = TempContentFiles::default();

                if is_truncated {
                    files = files.stdout(
                        self.create_temp_file("forge_fetch_", ".txt", &output.content)
                            .await?,
                    );
                }

                Ok(files)
            }
            ToolOperation::Shell { output } => {
                let config = self.services.get_config()?;
                let stdout_lines = output.output.stdout.lines().count();
                let stderr_lines = output.output.stderr.lines().count();
                let stdout_truncated =
                    stdout_lines > config.max_stdout_prefix_lines + config.max_stdout_suffix_lines;
                let stderr_truncated =
                    stderr_lines > config.max_stdout_prefix_lines + config.max_stdout_suffix_lines;

                let mut files = TempContentFiles::default();

                if stdout_truncated {
                    files = files.stdout(
                        self.create_temp_file("forge_shell_stdout_", ".txt", &output.output.stdout)
                            .await?,
                    );
                }
                if stderr_truncated {
                    files = files.stderr(
                        self.create_temp_file("forge_shell_stderr_", ".txt", &output.output.stderr)
                            .await?,
                    );
                }

                Ok(files)
            }
            ToolOperation::ShellPoll { output } => {
                let config = self.services.get_config()?;
                let stdout_lines = output.output.output.stdout.lines().count();
                let stderr_lines = output.output.output.stderr.lines().count();
                let stdout_truncated =
                    stdout_lines > config.max_stdout_prefix_lines + config.max_stdout_suffix_lines;
                let stderr_truncated =
                    stderr_lines > config.max_stdout_prefix_lines + config.max_stdout_suffix_lines;

                let mut files = TempContentFiles::default();

                if stdout_truncated {
                    files = files.stdout(
                        self.create_temp_file(
                            "forge_shell_stdout_",
                            ".txt",
                            &output.output.output.stdout,
                        )
                        .await?,
                    );
                }
                if stderr_truncated {
                    files = files.stderr(
                        self.create_temp_file(
                            "forge_shell_stderr_",
                            ".txt",
                            &output.output.output.stderr,
                        )
                        .await?,
                    );
                }

                Ok(files)
            }
            ToolOperation::ShellWait { output } => {
                let config = self.services.get_config()?;
                let stdout_lines = output.output.output.stdout.lines().count();
                let stderr_lines = output.output.output.stderr.lines().count();
                let stdout_truncated =
                    stdout_lines > config.max_stdout_prefix_lines + config.max_stdout_suffix_lines;
                let stderr_truncated =
                    stderr_lines > config.max_stdout_prefix_lines + config.max_stdout_suffix_lines;

                let mut files = TempContentFiles::default();

                if stdout_truncated {
                    files = files.stdout(
                        self.create_temp_file(
                            "forge_shell_stdout_",
                            ".txt",
                            &output.output.output.stdout,
                        )
                        .await?,
                    );
                }
                if stderr_truncated {
                    files = files.stderr(
                        self.create_temp_file(
                            "forge_shell_stderr_",
                            ".txt",
                            &output.output.output.stderr,
                        )
                        .await?,
                    );
                }

                Ok(files)
            }
            _ => Ok(TempContentFiles::default()),
        }
    }

    /// Converts a path to absolute by joining it with the current working
    /// directory if it's relative
    fn normalize_path(&self, path: String) -> String {
        let env = self.services.get_environment();
        let path_buf = PathBuf::from(&path);

        if path_buf.is_absolute() {
            path
        } else {
            PathBuf::from(&env.cwd).join(path_buf).display().to_string()
        }
    }

    async fn create_temp_file(
        &self,
        prefix: &str,
        ext: &str,
        content: &str,
    ) -> anyhow::Result<std::path::PathBuf> {
        let path = tempfile::Builder::new()
            .disable_cleanup(true)
            .prefix(prefix)
            .suffix(ext)
            .tempfile()?
            .into_temp_path()
            .to_path_buf();
        self.services
            .write(
                path.to_string_lossy().to_string(),
                content.to_string(),
                true,
            )
            .await?;
        Ok(path)
    }

    async fn call_internal(
        &self,
        input: ToolCatalog,
        context: &ToolCallContext,
    ) -> anyhow::Result<ToolOperation> {
        Ok(match input {
            ToolCatalog::Read(input) => {
                let normalized_path = self.normalize_path(input.file_path.clone());
                let output = self
                    .services
                    .read(
                        normalized_path,
                        input.start_line.map(|i| i as u64),
                        input.end_line.map(|i| i as u64),
                    )
                    .await?;

                (input, output).into()
            }
            ToolCatalog::Write(input) => {
                let normalized_path = self.normalize_path(input.file_path.clone());
                let output = self
                    .services
                    .write(normalized_path, input.content.clone(), input.overwrite)
                    .await?;
                (input, output).into()
            }
            ToolCatalog::FsSearch(input) => {
                let mut params = input.clone();
                // Normalize path if provided
                if let Some(ref path) = params.path {
                    params.path = Some(self.normalize_path(path.clone()));
                }
                let output = self.services.search(params).await?;
                (input, output).into()
            }
            ToolCatalog::SemSearch(input) => {
                let config = self.services.get_config()?;
                let env = self.services.get_environment();
                let services = self.services.clone();
                let cwd = env.cwd.clone();
                let limit = config.max_sem_search_results;
                let top_k = config.sem_search_top_k as u32;
                let params: Vec<_> = input
                    .queries
                    .iter()
                    .map(|search_query| {
                        artemis_domain::SearchParams::new(&search_query.query, &search_query.use_case)
                            .limit(limit)
                            .top_k(top_k)
                    })
                    .collect();

                // Execute all queries in parallel
                let futures: Vec<_> = params
                    .into_iter()
                    .map(|param| services.query_workspace(cwd.clone(), param))
                    .collect();

                let mut results = futures::future::try_join_all(futures).await?;

                // Deduplicate results across queries
                crate::search_dedup::deduplicate_results(&mut results);

                let output = input
                    .queries
                    .into_iter()
                    .zip(results)
                    .map(|(query, results)| CodebaseQueryResult {
                        query: query.query,
                        use_case: query.use_case,
                        results,
                    })
                    .collect::<Vec<_>>();

                let output = artemis_domain::CodebaseSearchResults { queries: output };
                ToolOperation::CodebaseSearch { output }
            }
            ToolCatalog::Remove(input) => {
                let normalized_path = self.normalize_path(input.path.clone());
                let output = self.services.remove(normalized_path).await?;
                (input, output).into()
            }
            ToolCatalog::Patch(input) => {
                let normalized_path = self.normalize_path(input.file_path.clone());
                let output = self
                    .services
                    .patch(
                        normalized_path,
                        input.old_string.clone(),
                        input.new_string.clone(),
                        input.replace_all,
                    )
                    .await?;
                (input, output).into()
            }
            ToolCatalog::MultiPatch(input) => {
                let normalized_path = self.normalize_path(input.file_path.clone());
                let output = self
                    .services
                    .multi_patch(normalized_path, input.edits.clone())
                    .await?;
                (input, output).into()
            }
            ToolCatalog::Undo(input) => {
                let normalized_path = self.normalize_path(input.path.clone());
                let output = self.services.undo(normalized_path).await?;
                (input, output).into()
            }
            ToolCatalog::Shell(input) => {
                // Intercept if the LLM mistakenly calls a tool name as a bash command.
                // e.g. shell("shell_poll 1") → execute poll(1) directly and return
                // the result transparently so the LLM gets the data it needs.
                let lowered = input.command.trim().to_ascii_lowercase();
                if let Some(job_id) = parse_tool_as_command("shell_poll", &lowered) {
                    tracing::info!(
                        job_id,
                        "Intercepting shell_poll called as bash command — executing poll directly"
                    );
                    let output = self.services.poll(job_id, false).await?;
                    return Ok(ToolOperation::ShellPoll { output });
                }
                if let Some(job_id) = parse_tool_as_command("shell_wait", &lowered) {
                    tracing::info!(
                        job_id,
                        "Intercepting shell_wait called as bash command — executing wait directly"
                    );
                    let output = self.services.wait(job_id, None, false).await?;
                    return Ok(ToolOperation::ShellWait { output });
                }
                if let Some(job_id) = parse_tool_as_command("shell_kill", &lowered) {
                    tracing::info!(
                        job_id,
                        "Intercepting shell_kill called as bash command — executing kill directly"
                    );
                    let output = self.services.kill(job_id).await?;
                    return Ok(ToolOperation::ShellKill { output });
                }

                validate_shell_guardrail(&input.command)?;
                let cwd = input
                    .cwd
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| self.services.get_environment().cwd.display().to_string());
                let normalized_cwd = self.normalize_path(cwd);
                let output = self
                    .services
                    .start(
                        input.command.clone(),
                        PathBuf::from(normalized_cwd),
                        input.env.clone(),
                        input.description.clone(),
                    )
                    .await?;
                ToolOperation::ShellStart { output }
            }
            ToolCatalog::ShellExec(input) => {
                validate_shell_guardrail(&input.command)?;
                let cwd = input
                    .cwd
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| self.services.get_environment().cwd.display().to_string());
                let normalized_cwd = self.normalize_path(cwd);
                let output = self
                    .services
                    .execute(
                        input.command.clone(),
                        PathBuf::from(normalized_cwd),
                        input.keep_ansi,
                        false,
                        input.env.clone(),
                        input.description.clone(),
                    )
                    .await?;
                ToolOperation::Shell { output }
            }
            ToolCatalog::ShellPoll(input) => {
                let output = self.services.poll(input.job_id, input.keep_ansi).await?;
                ToolOperation::ShellPoll { output }
            }
            ToolCatalog::ShellWait(input) => {
                let output = self
                    .services
                    .wait(input.job_id, input.timeout_ms, input.keep_ansi)
                    .await?;
                ToolOperation::ShellWait { output }
            }
            ToolCatalog::ShellKill(input) => {
                let output = self.services.kill(input.job_id).await?;
                ToolOperation::ShellKill { output }
            }
            ToolCatalog::Fetch(input) => {
                let output = self.services.fetch(input.url.clone(), input.raw).await?;
                (input, output).into()
            }
            ToolCatalog::Followup(input) => {
                let output = self
                    .services
                    .follow_up(
                        input.question.clone(),
                        input
                            .option1
                            .clone()
                            .into_iter()
                            .chain(input.option2.clone())
                            .chain(input.option3.clone())
                            .chain(input.option4.clone())
                            .chain(input.option5.clone())
                            .collect(),
                        input.multiple,
                    )
                    .await?;
                output.into()
            }
            ToolCatalog::Plan(input) => {
                let output = self
                    .services
                    .create_plan(
                        input.plan_name.clone(),
                        input.version.clone(),
                        input.content.clone(),
                    )
                    .await?;
                (input, output).into()
            }
            ToolCatalog::Skill(input) => {
                let skill = self.services.fetch_skill(input.name.clone()).await?;
                ToolOperation::Skill { output: skill }
            }
            ToolCatalog::TodoWrite(input) => {
                let before = context.get_todos()?;
                context.update_todos(input.todos.clone())?;
                let after = context.get_todos()?;
                ToolOperation::TodoWrite { before, after }
            }
            ToolCatalog::TodoRead(_input) => {
                let todos = context.get_todos()?;
                ToolOperation::TodoRead { output: todos }
            }
            ToolCatalog::Task(_) => {
                // Task tools are handled in ToolRegistry before reaching here
                unreachable!("Task tool should be handled in ToolRegistry")
            }
        })
    }

    pub async fn execute(
        &self,
        tool_input: ToolCatalog,
        context: &ToolCallContext,
    ) -> anyhow::Result<ToolOutput> {
        let tool_kind = tool_input.kind();
        let env = self.services.get_environment();
        let config = self.services.get_config()?;

        // Enforce read-before-edit for patch operations
        let file_path = match &tool_input {
            ToolCatalog::Patch(input) => Some(&input.file_path),
            ToolCatalog::MultiPatch(input) => Some(&input.file_path),
            _ => None,
        };

        if let Some(path) = file_path {
            self.require_prior_read(context, path, "edit it")?;
        }

        // Enforce read-before-edit for overwrite writes
        if let ToolCatalog::Write(input) = &tool_input
            && input.overwrite
        {
            self.require_prior_read(context, &input.file_path, "overwrite it")?;
        }

        let execution_result = self.call_internal(tool_input.clone(), context).await;

        if let Err(ref error) = execution_result {
            tracing::error!(error = ?error, "Tool execution failed");
        }

        let operation = execution_result?;

        // Send formatted output message
        if let Some(output) = operation.to_content(&env) {
            context.send(output).await?;
        }

        let truncation_path = self.dump_operation(&operation).await?;

        context.with_metrics(|metrics| {
            operation.into_tool_output(tool_kind, truncation_path, &env, &config, metrics)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::validate_shell_guardrail;

    #[test]
    fn test_validate_shell_guardrail_rejects_wait() {
        let actual = validate_shell_guardrail("wait");

        assert!(actual.is_err());
        assert!(actual.unwrap_err().to_string().contains("shell_wait"));
    }

    #[test]
    fn test_validate_shell_guardrail_rejects_sleep_monitoring() {
        let actual = validate_shell_guardrail("sleep 30 && cat /tmp/nmap.txt");

        assert!(actual.is_err());
        assert!(actual.unwrap_err().to_string().contains("shell_poll"));
    }

    #[test]
    fn test_validate_shell_guardrail_rejects_shell_poll_as_command() {
        let actual = validate_shell_guardrail("shell_poll 1");

        assert!(actual.is_err());
        let msg = actual.unwrap_err().to_string();
        assert!(msg.contains("tool name"), "Expected 'tool name' in: {msg}");
    }

    #[test]
    fn test_validate_shell_guardrail_rejects_shell_wait_as_command() {
        let actual = validate_shell_guardrail("shell_wait 1");

        assert!(actual.is_err());
        let msg = actual.unwrap_err().to_string();
        assert!(msg.contains("tool name"), "Expected 'tool name' in: {msg}");
    }

    #[test]
    fn test_validate_shell_guardrail_rejects_shell_kill_as_command() {
        let actual = validate_shell_guardrail("shell_kill 1");

        assert!(actual.is_err());
        let msg = actual.unwrap_err().to_string();
        assert!(msg.contains("tool name"), "Expected 'tool name' in: {msg}");
    }

    #[test]
    fn test_validate_shell_guardrail_rejects_shell_exec_as_command() {
        let actual = validate_shell_guardrail("shell_exec nslookup foo.com");

        assert!(actual.is_err());
        let msg = actual.unwrap_err().to_string();
        assert!(msg.contains("tool name"), "Expected 'tool name' in: {msg}");
    }

    #[test]
    fn test_validate_shell_guardrail_allows_valid_commands() {
        assert!(validate_shell_guardrail("nmap -sV scanme.nmap.org").is_ok());
        assert!(validate_shell_guardrail("nslookup google.com").is_ok());
        assert!(validate_shell_guardrail("git status").is_ok());
    }
}
