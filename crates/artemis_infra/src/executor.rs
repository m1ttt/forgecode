use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::{collections::HashMap, time::Duration};

use artemis_app::CommandInfra;
use artemis_domain::{
    CommandJobSnapshot, CommandKill, CommandOutput, CommandStart, ConsoleWriter as OutputPrinterTrait,
    Environment,
};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::{Mutex, Notify, oneshot};

use crate::console::StdConsoleWriter;

#[derive(Debug)]
struct JobState {
    command: String,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exit_code: Option<i32>,
    running: bool,
}

impl JobState {
    fn snapshot(&self, job_id: u64) -> CommandJobSnapshot {
        CommandJobSnapshot {
            job_id,
            output: CommandOutput {
                command: self.command.clone(),
                stdout: String::from_utf8_lossy(&self.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&self.stderr).into_owned(),
                exit_code: self.exit_code,
            },
            running: self.running,
        }
    }
}

#[derive(Debug)]
struct ShellJob {
    state: Arc<Mutex<JobState>>,
    done: Arc<Notify>,
    kill_tx: Mutex<Option<oneshot::Sender<()>>>,
}

/// Service for executing shell commands
#[derive(Clone, Debug)]
pub struct ForgeCommandExecutorService {
    env: Environment,
    output_printer: Arc<StdConsoleWriter>,

    // Mutex to ensure that only one command is executed at a time
    ready: Arc<Mutex<()>>,
    jobs: Arc<Mutex<HashMap<u64, Arc<ShellJob>>>>,
    next_job_id: Arc<AtomicU64>,
}

impl ForgeCommandExecutorService {
    pub fn new(env: Environment, output_printer: Arc<StdConsoleWriter>) -> Self {
        Self {
            env,
            output_printer,
            ready: Arc::new(Mutex::new(())),
            jobs: Arc::new(Mutex::new(HashMap::new())),
            next_job_id: Arc::new(AtomicU64::new(1)),
        }
    }

    fn prepare_command(
        &self,
        command_str: &str,
        working_dir: &Path,
        env_vars: Option<Vec<String>>,
        inherit_stdin: bool,
    ) -> Command {
        // Create a basic command
        let is_windows = cfg!(target_os = "windows");
        let shell = self.env.shell.as_str();
        let mut command = Command::new(shell);

        // Core color settings for general commands
        command
            .env("CLICOLOR_FORCE", "1")
            .env("FORCE_COLOR", "true")
            .env_remove("NO_COLOR");

        // Language/program specific color settings
        command
            .env("SBT_OPTS", "-Dsbt.color=always")
            .env("JAVA_OPTS", "-Dsbt.color=always");

        // enabled Git colors
        command.env("GIT_CONFIG_PARAMETERS", "'color.ui=always'");

        // Other common tools
        command.env("GREP_OPTIONS", "--color=always"); // GNU grep

        let parameter = if is_windows { "/C" } else { "-c" };
        command.arg(parameter);

        #[cfg(windows)]
        command.raw_arg(command_str);
        #[cfg(unix)]
        command.arg(command_str);

        tracing::info!(command = command_str, "Executing command");

        command.kill_on_drop(true);

        // Set the working directory
        command.current_dir(working_dir);

        // Configure the command for output
        command.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        if inherit_stdin {
            command.stdin(std::process::Stdio::inherit());
        } else {
            // Background jobs must not hold the interactive terminal input,
            // otherwise the TUI cannot keep accepting user messages.
            command.stdin(std::process::Stdio::null());
        }

        // Set requested environment variables
        if let Some(env_vars) = env_vars {
            for env_var in env_vars {
                if let Ok(value) = std::env::var(&env_var) {
                    command.env(&env_var, value);
                    tracing::debug!(env_var = %env_var, "Set environment variable from system");
                } else {
                    tracing::warn!(env_var = %env_var, "Environment variable not found in system");
                }
            }
        }

        command
    }

    /// Internal method to execute commands with streaming to console
    async fn execute_command_internal(
        &self,
        command: String,
        working_dir: &Path,
        silent: bool,
        env_vars: Option<Vec<String>>,
    ) -> anyhow::Result<CommandOutput> {
        let ready = self.ready.lock().await;

        let mut prepared_command = self.prepare_command(&command, working_dir, env_vars, true);

        // Spawn the command
        let mut child = prepared_command.spawn()?;

        let mut stdout_pipe = child.stdout.take();
        let mut stderr_pipe = child.stderr.take();

        // Stream the output of the command to stdout and stderr concurrently
        let (status, stdout_buffer, stderr_buffer) = if silent {
            tokio::try_join!(
                child.wait(),
                stream(&mut stdout_pipe, io::sink()),
                stream(&mut stderr_pipe, io::sink())
            )?
        } else {
            let stdout_writer = OutputPrinterWriter::stdout(self.output_printer.clone());
            let stderr_writer = OutputPrinterWriter::stderr(self.output_printer.clone());
            let result = tokio::try_join!(
                child.wait(),
                stream(&mut stdout_pipe, stdout_writer),
                stream(&mut stderr_pipe, stderr_writer)
            )?;

            // If the command's stdout did not end with a newline, the terminal
            // cursor is left mid-line. Write a newline so that subsequent output
            // (e.g. the LLM response) starts on a fresh line.
            if result.1.last() != Some(&b'\n') && !result.1.is_empty() {
                let _ = self.output_printer.write(b"\n");
                let _ = self.output_printer.flush();
            }

            result
        };

        // Drop happens after `try_join` due to <https://github.com/tokio-rs/tokio/issues/4309>
        drop(stdout_pipe);
        drop(stderr_pipe);
        drop(ready);

        Ok(CommandOutput {
            stdout: String::from_utf8_lossy(&stdout_buffer).into_owned(),
            stderr: String::from_utf8_lossy(&stderr_buffer).into_owned(),
            exit_code: status.code(),
            command,
        })
    }

    async fn start_command_internal(
        &self,
        command: String,
        working_dir: &Path,
        env_vars: Option<Vec<String>>,
    ) -> anyhow::Result<CommandStart> {
        let jobs: Vec<(u64, Arc<ShellJob>)> = self
            .jobs
            .lock()
            .await
            .iter()
            .map(|(job_id, job)| (*job_id, Arc::clone(job)))
            .collect();
        for (job_id, job) in jobs {
            let state = job.state.lock().await;
            if state.running {
                anyhow::bail!(
                    "Shell job {} is already running for command `{}`. Use shell_poll, shell_wait, or shell_kill before starting another command.",
                    job_id,
                    state.command
                );
            }
        }

        let mut prepared_command = self.prepare_command(&command, working_dir, env_vars, false);
        let mut child = prepared_command.spawn()?;
        let mut stdout_pipe = child.stdout.take();
        let mut stderr_pipe = child.stderr.take();
        let job_id = self.next_job_id.fetch_add(1, Ordering::Relaxed);

        let state = Arc::new(Mutex::new(JobState {
            command: command.clone(),
            stdout: Vec::new(),
            stderr: Vec::new(),
            exit_code: None,
            running: true,
        }));
        let done = Arc::new(Notify::new());
        let (kill_tx, mut kill_rx) = oneshot::channel::<()>();

        let state_for_stdout = state.clone();
        let state_for_stderr = state.clone();
        let state_for_task = state.clone();
        let done_for_task = done.clone();

        tokio::spawn(async move {
            let stdout_task = tokio::spawn(async move {
                stream_to_state(&mut stdout_pipe, state_for_stdout, true).await
            });
            let stderr_task = tokio::spawn(async move {
                stream_to_state(&mut stderr_pipe, state_for_stderr, false).await
            });

            let wait_result = tokio::select! {
                _ = &mut kill_rx => {
                    let _ = child.kill().await;
                    child.wait().await
                }
                status = child.wait() => status,
            };

            let _ = stdout_task.await;
            let _ = stderr_task.await;

            let mut state = state_for_task.lock().await;
            match wait_result {
                Ok(status) => state.exit_code = status.code(),
                Err(err) => {
                    state
                        .stderr
                        .extend_from_slice(format!("\n[forge] failed waiting for process: {err}\n").as_bytes());
                    state.exit_code = Some(1);
                }
            }
            state.running = false;
            drop(state);
            done_for_task.notify_waiters();
        });

        self.jobs.lock().await.insert(
            job_id,
            Arc::new(ShellJob {
                state,
                done,
                kill_tx: Mutex::new(Some(kill_tx)),
            }),
        );

        Ok(CommandStart { job_id, command })
    }

    async fn get_job(&self, job_id: u64) -> anyhow::Result<Arc<ShellJob>> {
        self.jobs
            .lock()
            .await
            .get(&job_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Shell job {job_id} not found"))
    }

    async fn poll_command_internal(&self, job_id: u64) -> anyhow::Result<CommandJobSnapshot> {
        let job = self.get_job(job_id).await?;
        let state = job.state.lock().await;
        Ok(state.snapshot(job_id))
    }

    async fn wait_command_internal(
        &self,
        job_id: u64,
        timeout_ms: Option<u64>,
    ) -> anyhow::Result<CommandJobSnapshot> {
        let job = self.get_job(job_id).await?;
        let wait_for_completion = async {
            loop {
                let running = job.state.lock().await.running;
                if !running {
                    break;
                }
                job.done.notified().await;
            }
        };

        if let Some(timeout_ms) = timeout_ms {
            let _ = tokio::time::timeout(Duration::from_millis(timeout_ms), wait_for_completion).await;
        } else {
            wait_for_completion.await;
        }

        self.poll_command_internal(job_id).await
    }

    async fn kill_command_internal(&self, job_id: u64) -> anyhow::Result<CommandKill> {
        let job = self.get_job(job_id).await?;
        let mut kill_tx = job.kill_tx.lock().await;
        let killed = kill_tx.take().is_some_and(|tx| tx.send(()).is_ok());
        Ok(CommandKill { job_id, killed })
    }
}

/// Writer that delegates to OutputPrinter for synchronized writes.
struct OutputPrinterWriter {
    printer: Arc<StdConsoleWriter>,
    is_stdout: bool,
}

impl OutputPrinterWriter {
    fn stdout(printer: Arc<StdConsoleWriter>) -> Self {
        Self { printer, is_stdout: true }
    }

    fn stderr(printer: Arc<StdConsoleWriter>) -> Self {
        Self { printer, is_stdout: false }
    }
}

impl Write for OutputPrinterWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.is_stdout {
            self.printer.write(buf)
        } else {
            self.printer.write_err(buf)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.is_stdout {
            self.printer.flush()
        } else {
            self.printer.flush_err()
        }
    }
}

/// reads the output from A and writes it to W
async fn stream<A: AsyncReadExt + Unpin, W: Write>(
    io: &mut Option<A>,
    mut writer: W,
) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    if let Some(io) = io.as_mut() {
        let mut buff = [0; 1024];
        loop {
            let n = io.read(&mut buff).await?;
            if n == 0 {
                break;
            }
            writer.write_all(&buff[..n])?;
            // note: flush is necessary else we get the cursor could not be found error.
            writer.flush()?;
            output.extend_from_slice(&buff[..n]);
        }
    }
    Ok(output)
}

async fn stream_to_state<A: AsyncReadExt + Unpin>(
    io: &mut Option<A>,
    state: Arc<Mutex<JobState>>,
    is_stdout: bool,
) -> io::Result<()> {
    if let Some(io) = io.as_mut() {
        let mut buff = [0; 1024];
        loop {
            let n = io.read(&mut buff).await?;
            if n == 0 {
                break;
            }
            let mut state = state.lock().await;
            if is_stdout {
                state.stdout.extend_from_slice(&buff[..n]);
            } else {
                state.stderr.extend_from_slice(&buff[..n]);
            }
        }
    }
    Ok(())
}

/// The implementation for CommandExecutorService
#[async_trait::async_trait]
impl CommandInfra for ForgeCommandExecutorService {
    async fn execute_command(
        &self,
        command: String,
        working_dir: PathBuf,
        silent: bool,
        env_vars: Option<Vec<String>>,
    ) -> anyhow::Result<CommandOutput> {
        self.execute_command_internal(command, &working_dir, silent, env_vars)
            .await
    }

    async fn execute_command_raw(
        &self,
        command: &str,
        working_dir: PathBuf,
        env_vars: Option<Vec<String>>,
    ) -> anyhow::Result<std::process::ExitStatus> {
        let mut prepared_command = self.prepare_command(command, &working_dir, env_vars, true);

        // overwrite the stdin, stdout and stderr to inherit
        prepared_command
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit());

        Ok(prepared_command.spawn()?.wait().await?)
    }

    async fn start_command(
        &self,
        command: String,
        working_dir: PathBuf,
        env_vars: Option<Vec<String>>,
    ) -> anyhow::Result<CommandStart> {
        self.start_command_internal(command, &working_dir, env_vars)
            .await
    }

    async fn poll_command(&self, job_id: u64) -> anyhow::Result<CommandJobSnapshot> {
        self.poll_command_internal(job_id).await
    }

    async fn wait_command(
        &self,
        job_id: u64,
        timeout_ms: Option<u64>,
    ) -> anyhow::Result<CommandJobSnapshot> {
        self.wait_command_internal(job_id, timeout_ms).await
    }

    async fn kill_command(&self, job_id: u64) -> anyhow::Result<CommandKill> {
        self.kill_command_internal(job_id).await
    }
}

#[cfg(test)]
mod tests {

    use pretty_assertions::assert_eq;

    use super::*;

    fn test_env() -> Environment {
        use fake::{Fake, Faker};
        let fixture: Environment = Faker.fake();
        fixture.shell(
            if cfg!(target_os = "windows") {
                "cmd"
            } else {
                "bash"
            }
            .to_string(),
        )
    }

    fn test_printer() -> Arc<StdConsoleWriter> {
        Arc::new(StdConsoleWriter::default())
    }

    #[tokio::test]
    async fn test_command_executor() {
        let fixture = ForgeCommandExecutorService::new(test_env(), test_printer());
        let cmd = "echo 'hello world'";
        let dir = ".";

        let actual = fixture
            .execute_command(cmd.to_string(), PathBuf::new().join(dir), false, None)
            .await
            .unwrap();

        let mut expected = CommandOutput {
            stdout: "hello world\n".to_string(),
            stderr: "".to_string(),
            command: "echo \"hello world\"".into(),
            exit_code: Some(0),
        };

        if cfg!(target_os = "windows") {
            expected.stdout = format!("'{}'", expected.stdout);
        }

        assert_eq!(actual.stdout.trim(), expected.stdout.trim());
        assert_eq!(actual.stderr, expected.stderr);
        assert_eq!(actual.success(), expected.success());
    }
    #[tokio::test]
    async fn test_command_executor_with_env_vars_success() {
        // Set up test environment variables
        unsafe {
            std::env::set_var("TEST_ENV_VAR", "test_value");
            std::env::set_var("ANOTHER_TEST_VAR", "another_value");
        }

        let fixture = ForgeCommandExecutorService::new(test_env(), test_printer());
        let cmd = if cfg!(target_os = "windows") {
            "echo %TEST_ENV_VAR%"
        } else {
            "echo $TEST_ENV_VAR"
        };

        let actual = fixture
            .execute_command(
                cmd.to_string(),
                PathBuf::new().join("."),
                false,
                Some(vec!["TEST_ENV_VAR".to_string()]),
            )
            .await
            .unwrap();

        assert!(actual.success());
        assert!(actual.stdout.contains("test_value"));

        // Clean up
        unsafe {
            std::env::remove_var("TEST_ENV_VAR");
            std::env::remove_var("ANOTHER_TEST_VAR");
        }
    }

    #[tokio::test]
    async fn test_command_executor_with_missing_env_vars() {
        unsafe {
            std::env::remove_var("MISSING_ENV_VAR");
        }

        let fixture = ForgeCommandExecutorService::new(test_env(), test_printer());
        let cmd = if cfg!(target_os = "windows") {
            "echo %MISSING_ENV_VAR%"
        } else {
            "echo ${MISSING_ENV_VAR:-default_value}"
        };

        let actual = fixture
            .execute_command(
                cmd.to_string(),
                PathBuf::new().join("."),
                false,
                Some(vec!["MISSING_ENV_VAR".to_string()]),
            )
            .await
            .unwrap();

        // Should still succeed even with missing env vars
        assert!(actual.success());
    }

    #[tokio::test]
    async fn test_command_executor_with_empty_env_list() {
        let fixture = ForgeCommandExecutorService::new(test_env(), test_printer());
        let cmd = "echo 'no env vars'";

        let actual = fixture
            .execute_command(
                cmd.to_string(),
                PathBuf::new().join("."),
                false,
                Some(vec![]),
            )
            .await
            .unwrap();

        assert!(actual.success());
        assert!(actual.stdout.contains("no env vars"));
    }

    #[tokio::test]
    async fn test_command_executor_with_multiple_env_vars() {
        unsafe {
            std::env::set_var("FIRST_VAR", "first");
            std::env::set_var("SECOND_VAR", "second");
        }

        let fixture = ForgeCommandExecutorService::new(test_env(), test_printer());
        let cmd = if cfg!(target_os = "windows") {
            "echo %FIRST_VAR% %SECOND_VAR%"
        } else {
            "echo $FIRST_VAR $SECOND_VAR"
        };

        let actual = fixture
            .execute_command(
                cmd.to_string(),
                PathBuf::new().join("."),
                false,
                Some(vec!["FIRST_VAR".to_string(), "SECOND_VAR".to_string()]),
            )
            .await
            .unwrap();

        assert!(actual.success());
        assert!(actual.stdout.contains("first"));
        assert!(actual.stdout.contains("second"));

        // Clean up
        unsafe {
            std::env::remove_var("FIRST_VAR");
            std::env::remove_var("SECOND_VAR");
        }
    }

    #[tokio::test]
    async fn test_command_executor_silent() {
        let fixture = ForgeCommandExecutorService::new(test_env(), test_printer());
        let cmd = "echo 'silent test'";
        let dir = ".";

        let actual = fixture
            .execute_command(cmd.to_string(), PathBuf::new().join(dir), true, None)
            .await
            .unwrap();

        let mut expected = CommandOutput {
            stdout: "silent test\n".to_string(),
            stderr: "".to_string(),
            command: "echo \"silent test\"".into(),
            exit_code: Some(0),
        };

        if cfg!(target_os = "windows") {
            expected.stdout = format!("'{}'", expected.stdout);
        }

        // The output should still be captured in the CommandOutput
        assert_eq!(actual.stdout.trim(), expected.stdout.trim());
        assert_eq!(actual.stderr, expected.stderr);
        assert_eq!(actual.success(), expected.success());
    }

    #[tokio::test]
    async fn test_start_and_wait_command() {
        let service = ForgeCommandExecutorService::new(test_env(), test_printer());
        let start = service
            .start_command("echo 'async hello'".to_string(), PathBuf::from("."), None)
            .await
            .unwrap();
        let waited = service.wait_command(start.job_id, None).await.unwrap();

        assert_eq!(waited.job_id, start.job_id);
        assert!(!waited.running);
        assert_eq!(waited.output.exit_code, Some(0));
        assert!(waited.output.stdout.contains("async hello"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_start_and_kill_command() {
        let service = ForgeCommandExecutorService::new(test_env(), test_printer());
        let start = service
            .start_command("sleep 5".to_string(), PathBuf::from("."), None)
            .await
            .unwrap();

        let killed = service.kill_command(start.job_id).await.unwrap();
        assert_eq!(killed.job_id, start.job_id);
        assert!(killed.killed);

        let waited = service
            .wait_command(start.job_id, Some(2000))
            .await
            .unwrap();
        assert!(!waited.running);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_start_command_rejects_second_running_job() {
        let fixture = ForgeCommandExecutorService::new(test_env(), test_printer());
        let first = fixture
            .start_command("sleep 5".to_string(), PathBuf::from("."), None)
            .await
            .unwrap();

        let actual = fixture
            .start_command("echo 'second'".to_string(), PathBuf::from("."), None)
            .await;

        assert!(actual.is_err());
        assert!(actual
            .unwrap_err()
            .to_string()
            .contains("Use shell_poll, shell_wait, or shell_kill"));

        let _ = fixture.kill_command(first.job_id).await.unwrap();
    }
}
