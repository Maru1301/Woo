use std::{
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    time::timeout,
};

/// Keep Woo-owned console executables invisible when launched by the Windows
/// release GUI process. Tokio's Command exposes the Windows creation flags
/// directly; all Git execution paths (and the cancellation helper) use this
/// factory so new commands inherit the same policy.
fn desktop_child(executable: &str) -> Command {
    let mut command = Command::new(executable);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug)]
pub struct GitOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: Option<i32>,
    pub duration: Duration,
}

impl GitOutput {
    pub fn success(&self) -> bool {
        self.exit_code == Some(0)
    }
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout)
            .trim_end_matches(['\r', '\n'])
            .to_string()
    }
}

pub struct GitRunner {
    timeout: Duration,
}

impl Default for GitRunner {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(10),
        }
    }
}

#[derive(Debug)]
pub enum GitRunError {
    MissingExecutable,
    Io(std::io::Error),
    Timeout,
    OutputLimit,
    Cancelled,
}

impl GitRunner {
    /// Remote commands can wait on a network. Capture diagnostics with fixed
    /// bounds while draining both pipes, and explicitly reap on cancellation.
    pub async fn run_remote(
        &self,
        directory: &Path,
        args: &[&str],
        mut cancelled: tokio::sync::watch::Receiver<bool>,
        max_duration: Option<Duration>,
    ) -> Result<GitOutput, GitRunError> {
        let started = Instant::now();
        let mut command = desktop_child("git");
        command
            .args(args)
            .current_dir(directory)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_INDEX_FILE")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                GitRunError::MissingExecutable
            } else {
                GitRunError::Io(error)
            }
        })?;
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let stdout_task = tokio::spawn(capture_bounded(stdout, 64 * 1024));
        let stderr_task = tokio::spawn(capture_bounded(stderr, 256 * 1024));
        let deadline = max_duration.unwrap_or(Duration::from_secs(365 * 24 * 60 * 60));
        let result = tokio::select! {
            biased;
            status = child.wait() => status.map_err(GitRunError::Io),
            _ = async { if !*cancelled.borrow() { let _ = cancelled.changed().await; } } => {
                match child.try_wait().map_err(GitRunError::Io)? {
                    Some(status) => Ok(status),
                    None => Err(GitRunError::Cancelled),
                }
            },
            _ = tokio::time::sleep(deadline) => {
                match child.try_wait().map_err(GitRunError::Io)? {
                    Some(status) => Ok(status),
                    None => Err(GitRunError::Timeout),
                }
            },
        };
        if result.is_err() {
            terminate_remote_child(&mut child).await?;
        }
        let stdout = tokio::time::timeout(Duration::from_secs(3), stdout_task)
            .await
            .map_err(|_| {
                GitRunError::Io(std::io::Error::other(
                    "Git stdout pipe did not close after process termination",
                ))
            })?
            .map_err(|e| GitRunError::Io(std::io::Error::other(e)))?
            .map_err(GitRunError::Io)?;
        let stderr = tokio::time::timeout(Duration::from_secs(3), stderr_task)
            .await
            .map_err(|_| {
                GitRunError::Io(std::io::Error::other(
                    "Git stderr pipe did not close after process termination",
                ))
            })?
            .map_err(|e| GitRunError::Io(std::io::Error::other(e)))?
            .map_err(GitRunError::Io)?;
        let status = result?;
        Ok(GitOutput {
            stdout,
            stderr,
            exit_code: status.code(),
            duration: started.elapsed(),
        })
    }

    pub fn with_timeout(timeout: Duration) -> Self {
        Self { timeout }
    }

    pub async fn run(&self, directory: &Path, args: &[&str]) -> Result<GitOutput, GitRunError> {
        self.run_with_input(directory, args, None).await
    }

    pub async fn run_with_input(
        &self,
        directory: &Path,
        args: &[&str],
        input: Option<&[u8]>,
    ) -> Result<GitOutput, GitRunError> {
        let started = Instant::now();
        let mut command = desktop_child("git");
        command
            .args(args)
            .current_dir(directory)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_INDEX_FILE")
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                GitRunError::MissingExecutable
            } else {
                GitRunError::Io(error)
            }
        })?;
        let output = timeout(self.timeout, async move {
            let mut input_error = None;
            if let Some(bytes) = input {
                let mut stdin = child.stdin.take().expect("piped Git stdin");
                if let Err(error) = stdin.write_all(bytes).await {
                    input_error = Some(error);
                } else if let Err(error) = stdin.shutdown().await {
                    input_error = Some(error);
                }
            }
            let output = child.wait_with_output().await?;
            if output.status.success() {
                if let Some(error) = input_error {
                    return Err(error);
                }
            }
            Ok(output)
        })
        .await
        .map_err(|_| GitRunError::Timeout)?
        .map_err(GitRunError::Io)?;
        Ok(GitOutput {
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: output.status.code(),
            duration: started.elapsed(),
        })
    }

    /// Bounded stdout for potentially large patch output. The child is killed
    /// once the limit is exceeded; stderr is drained concurrently with a cap.
    pub async fn run_limited(
        &self,
        directory: &Path,
        args: &[&str],
        max_stdout: usize,
    ) -> Result<GitOutput, GitRunError> {
        let started = Instant::now();
        let mut command = desktop_child("git");
        command
            .args(args)
            .current_dir(directory)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_INDEX_FILE")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                GitRunError::MissingExecutable
            } else {
                GitRunError::Io(error)
            }
        })?;
        let stdout = child.stdout.take().expect("piped Git stdout");
        let stderr = child.stderr.take().expect("piped Git stderr");
        let stderr_task = tokio::spawn(async move {
            let mut reader = stderr;
            let mut bytes = Vec::new();
            let mut buffer = [0u8; 8192];
            loop {
                let read = reader.read(&mut buffer).await?;
                if read == 0 {
                    break;
                }
                let remaining = (64 * 1024usize).saturating_sub(bytes.len());
                bytes.extend_from_slice(&buffer[..read.min(remaining)]);
            }
            Ok::<_, std::io::Error>(bytes)
        });
        let result = timeout(self.timeout, async {
            let mut output = Vec::new();
            stdout
                .take(max_stdout as u64 + 1)
                .read_to_end(&mut output)
                .await
                .map_err(GitRunError::Io)?;
            if output.len() > max_stdout {
                // The child may have exited after filling the pipe. Either way,
                // the response exceeded the limit and should use the same error.
                let _ = child.kill().await;
                let _ = child.wait().await;
                let _ = stderr_task.await;
                return Err(GitRunError::OutputLimit);
            }
            let status = child.wait().await.map_err(GitRunError::Io)?;
            let stderr = stderr_task
                .await
                .map_err(|error| GitRunError::Io(std::io::Error::other(error)))?
                .map_err(GitRunError::Io)?;
            Ok(GitOutput {
                stdout: output,
                stderr,
                exit_code: status.code(),
                duration: started.elapsed(),
            })
        })
        .await
        .map_err(|_| GitRunError::Timeout)?;
        result
    }
}

async fn terminate_remote_child(child: &mut tokio::process::Child) -> Result<(), GitRunError> {
    #[cfg(windows)]
    if let Some(pid) = child.id() {
        // Git may launch ssh, credential helpers, or hooks. Killing only git
        // leaves descendants holding pipe handles and can strand the operation.
        let _ = tokio::time::timeout(
            Duration::from_secs(5),
            desktop_child("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status(),
        )
        .await;
    }
    let _ = child.kill().await;
    child.wait().await.map_err(GitRunError::Io)?;
    Ok(())
}

async fn capture_bounded(
    mut reader: impl tokio::io::AsyncRead + Unpin,
    limit: usize,
) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(output.len());
        output.extend_from_slice(&buffer[..read.min(remaining)]);
    }
    Ok(output)
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetConsoleWindow() -> *mut std::ffi::c_void;
    }

    #[test]
    fn console_probe_child() {
        if std::env::var_os("WOO_CONSOLE_PROBE").is_some() {
            // This test binary is itself a console executable. A child created
            // with CREATE_NO_WINDOW must have no attached console even then.
            println!("console={}", unsafe { !GetConsoleWindow().is_null() });
        }
    }

    #[tokio::test]
    async fn desktop_child_has_no_console_and_keeps_stdout() {
        let output = desktop_child(std::env::current_exe().unwrap().to_str().unwrap())
            .args([
                "--exact",
                "git::windows_tests::console_probe_child",
                "--nocapture",
            ])
            .env("WOO_CONSOLE_PROBE", "1")
            .output()
            .await
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("console=false"),
            "{output:?}"
        );
    }
}
