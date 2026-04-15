use std::path::PathBuf;
use std::sync::Arc;

use anyhow::bail;
use artemis_app::domain::Environment;
use artemis_app::{
    CommandInfra, EnvironmentInfra, ShellKillOutput, ShellOutput, ShellPollOutput, ShellService,
    ShellStartOutput, ShellWaitOutput,
};
use strip_ansi_escapes::strip;

// Strips out the ansi codes from content.
fn strip_ansi(content: String) -> String {
    String::from_utf8_lossy(&strip(content.as_bytes())).into_owned()
}

/// Prevents potentially harmful operations like absolute path execution and
/// directory changes. Use for file system interaction, running utilities,
/// installing packages, or executing build commands. For operations requiring
/// unrestricted access, advise users to run forge CLI with '-u' flag. Returns
/// complete output including stdout, stderr, and exit code for diagnostic
/// purposes.
pub struct ForgeShell<I> {
    env: Environment,
    infra: Arc<I>,
}

impl<I: EnvironmentInfra> ForgeShell<I> {
    /// Create a new Shell with environment configuration
    pub fn new(infra: Arc<I>) -> Self {
        let env = infra.get_environment();
        Self { env, infra }
    }

    fn validate_command(command: &str) -> anyhow::Result<()> {
        let trimmed = command.trim();
        if trimmed.is_empty() {
            bail!("Command string is empty or contains only whitespace");
        }
        let lowered = trimmed.to_ascii_lowercase();
        if lowered == "wait" || lowered.starts_with("wait ") {
            bail!(
                "Do not use shell with `wait`. Reuse the background job `job_id` with shell_wait."
            );
        }
        if lowered == "jobs" || lowered.starts_with("jobs ") {
            bail!(
                "Do not use shell with `jobs`. Reuse the background job `job_id` with shell_poll."
            );
        }
        if lowered.starts_with("sleep ")
            || lowered.contains("&& sleep ")
            || lowered.contains("; sleep ")
        {
            bail!(
                "Do not use shell with `sleep` to monitor a background command. Return control to the user or reuse the existing `job_id` with shell_poll."
            );
        }
        if lowered.starts_with("ps ") && lowered.contains("grep") {
            bail!(
                "Do not use `ps ... | grep ...` to monitor a background shell command. Reuse the existing `job_id` with shell_poll."
            );
        }
        if lowered.starts_with("cat /tmp/") || lowered.contains("&& cat /tmp/") {
            bail!(
                "Do not use shell with `cat /tmp/...` to inspect background command output. Use the read tool for files or reuse the existing `job_id` with shell_poll/shell_wait."
            );
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl<I: CommandInfra + EnvironmentInfra> ShellService for ForgeShell<I> {
    async fn execute(
        &self,
        command: String,
        cwd: PathBuf,
        keep_ansi: bool,
        silent: bool,
        env_vars: Option<Vec<String>>,
        description: Option<String>,
    ) -> anyhow::Result<ShellOutput> {
        Self::validate_command(&command)?;

        let mut output = self
            .infra
            .execute_command(command, cwd, silent, env_vars)
            .await?;

        if !keep_ansi {
            output.stdout = strip_ansi(output.stdout);
            output.stderr = strip_ansi(output.stderr);
        }

        Ok(ShellOutput { output, shell: self.env.shell.clone(), description })
    }

    async fn start(
        &self,
        command: String,
        cwd: PathBuf,
        env_vars: Option<Vec<String>>,
        description: Option<String>,
    ) -> anyhow::Result<ShellStartOutput> {
        Self::validate_command(&command)?;
        let output = self.infra.start_command(command, cwd, env_vars).await?;
        Ok(ShellStartOutput { output, shell: self.env.shell.clone(), description })
    }

    async fn poll(&self, job_id: u64, keep_ansi: bool) -> anyhow::Result<ShellPollOutput> {
        let mut output = self.infra.poll_command(job_id).await?;
        if !keep_ansi {
            output.output.stdout = strip_ansi(output.output.stdout);
            output.output.stderr = strip_ansi(output.output.stderr);
        }
        Ok(ShellPollOutput { output, shell: self.env.shell.clone() })
    }

    async fn wait(
        &self,
        job_id: u64,
        timeout_ms: Option<u64>,
        keep_ansi: bool,
    ) -> anyhow::Result<ShellWaitOutput> {
        let mut output = self.infra.wait_command(job_id, timeout_ms).await?;
        if !keep_ansi {
            output.output.stdout = strip_ansi(output.output.stdout);
            output.output.stderr = strip_ansi(output.output.stderr);
        }
        Ok(ShellWaitOutput { output, shell: self.env.shell.clone() })
    }

    async fn kill(&self, job_id: u64) -> anyhow::Result<ShellKillOutput> {
        let output = self.infra.kill_command(job_id).await?;
        Ok(ShellKillOutput { output, shell: self.env.shell.clone() })
    }
}
#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use async_trait::async_trait;
    use artemis_app::domain::{CommandOutput, Environment};
    use artemis_app::{CommandInfra, EnvironmentInfra, ShellService};
    use artemis_domain::ConfigOperation;
    use pretty_assertions::assert_eq;

    use super::*;

    struct MockCommandInfra {
        expected_env_vars: Option<Vec<String>>,
    }

    #[async_trait]
    impl CommandInfra for MockCommandInfra {
        async fn execute_command(
            &self,
            command: String,
            _working_dir: PathBuf,
            _silent: bool,
            env_vars: Option<Vec<String>>,
        ) -> anyhow::Result<CommandOutput> {
            // Verify that environment variables are passed through correctly
            assert_eq!(env_vars, self.expected_env_vars);

            Ok(CommandOutput {
                stdout: "Mock output".to_string(),
                stderr: "".to_string(),
                command,
                exit_code: Some(0),
            })
        }

        async fn execute_command_raw(
            &self,
            _command: &str,
            _working_dir: PathBuf,
            _env_vars: Option<Vec<String>>,
        ) -> anyhow::Result<std::process::ExitStatus> {
            unimplemented!()
        }
    }

    impl EnvironmentInfra for MockCommandInfra {
        type Config = artemis_config::ForgeConfig;

        fn get_environment(&self) -> Environment {
            use fake::{Fake, Faker};
            Faker.fake()
        }

        fn get_config(&self) -> anyhow::Result<artemis_config::ForgeConfig> {
            Ok(artemis_config::ForgeConfig::default())
        }

        async fn update_environment(&self, _ops: Vec<ConfigOperation>) -> anyhow::Result<()> {
            unimplemented!()
        }

        fn get_env_var(&self, _key: &str) -> Option<String> {
            None
        }

        fn get_env_vars(&self) -> std::collections::BTreeMap<String, String> {
            std::collections::BTreeMap::new()
        }
    }

    #[tokio::test]
    async fn test_shell_service_forwards_env_vars() {
        let fixture = ForgeShell::new(Arc::new(MockCommandInfra {
            expected_env_vars: Some(vec!["PATH".to_string(), "HOME".to_string()]),
        }));

        let actual = fixture
            .execute(
                "echo hello".to_string(),
                PathBuf::from("."),
                false,
                false,
                Some(vec!["PATH".to_string(), "HOME".to_string()]),
                None,
            )
            .await
            .unwrap();

        assert_eq!(actual.output.stdout, "Mock output");
        assert_eq!(actual.output.exit_code, Some(0));
    }

    #[tokio::test]
    async fn test_shell_service_forwards_no_env_vars() {
        let fixture = ForgeShell::new(Arc::new(MockCommandInfra { expected_env_vars: None }));

        let actual = fixture
            .execute(
                "echo hello".to_string(),
                PathBuf::from("."),
                false,
                false,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(actual.output.stdout, "Mock output");
        assert_eq!(actual.output.exit_code, Some(0));
    }

    #[tokio::test]
    async fn test_shell_service_forwards_empty_env_vars() {
        let fixture = ForgeShell::new(Arc::new(MockCommandInfra {
            expected_env_vars: Some(vec![]),
        }));

        let actual = fixture
            .execute(
                "echo hello".to_string(),
                PathBuf::from("."),
                false,
                false,
                Some(vec![]),
                None,
            )
            .await
            .unwrap();

        assert_eq!(actual.output.stdout, "Mock output");
        assert_eq!(actual.output.exit_code, Some(0));
    }

    #[tokio::test]
    async fn test_shell_service_with_description() {
        let fixture = ForgeShell::new(Arc::new(MockCommandInfra { expected_env_vars: None }));

        let actual = fixture
            .execute(
                "echo hello".to_string(),
                PathBuf::from("."),
                false,
                false,
                None,
                Some("Prints hello to stdout".to_string()),
            )
            .await
            .unwrap();

        assert_eq!(actual.output.stdout, "Mock output");
        assert_eq!(actual.output.exit_code, Some(0));
        assert_eq!(
            actual.description,
            Some("Prints hello to stdout".to_string())
        );
    }

    #[tokio::test]
    async fn test_shell_service_without_description() {
        let fixture = ForgeShell::new(Arc::new(MockCommandInfra { expected_env_vars: None }));

        let actual = fixture
            .execute(
                "echo hello".to_string(),
                PathBuf::from("."),
                false,
                false,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(actual.output.stdout, "Mock output");
        assert_eq!(actual.output.exit_code, Some(0));
        assert_eq!(actual.description, None);
    }

    #[test]
    fn test_validate_command_rejects_wait() {
        let actual = ForgeShell::<MockCommandInfra>::validate_command("wait");

        assert!(actual.is_err());
        assert!(actual.unwrap_err().to_string().contains("shell_wait"));
    }

    #[test]
    fn test_validate_command_rejects_sleep_monitoring() {
        let actual =
            ForgeShell::<MockCommandInfra>::validate_command("sleep 30 && cat /tmp/nmap.txt");

        assert!(actual.is_err());
        assert!(actual.unwrap_err().to_string().contains("shell_poll"));
    }

    #[test]
    fn test_validate_command_rejects_ps_grep_monitoring() {
        let actual = ForgeShell::<MockCommandInfra>::validate_command("ps aux | grep nmap");

        assert!(actual.is_err());
        assert!(actual.unwrap_err().to_string().contains("shell_poll"));
    }
}
