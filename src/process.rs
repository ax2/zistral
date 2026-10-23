use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{
    OnceLock,
    atomic::{AtomicU32, Ordering},
};
use std::time::{Duration, Instant};

const OUTPUT_LIMIT: usize = 64 * 1024;
static ACTIVE: AtomicU32 = AtomicU32::new(0);

pub fn install_interrupt_handler() -> Result<()> {
    static INSTALLED: OnceLock<Result<(), String>> = OnceLock::new();
    INSTALLED
        .get_or_init(|| {
            ctrlc::set_handler(|| {
                #[cfg(unix)]
                {
                    let pid = ACTIVE.load(Ordering::SeqCst);
                    if pid != 0 {
                        unsafe {
                            libc::kill(-(pid as i32), libc::SIGKILL);
                        }
                    }
                }
                // Windows closes the job handle on process exit, terminating its members.
                std::process::exit(130);
            })
            .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|e| anyhow::anyhow!("cannot install interrupt handler: {e}"))?;
    Ok(())
}

pub fn shell(command: &str, cwd: &Path, timeout: u64) -> Result<Value> {
    if command.trim().is_empty() || command.len() > 32 * 1024 {
        bail!("command must contain 1-32768 bytes");
    }
    #[cfg(windows)]
    {
        let script = format!(
            "[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false); $OutputEncoding=[Console]::OutputEncoding; {command}"
        );
        let args = [
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &script,
        ];
        match run("pwsh.exe", &args, cwd, timeout) {
            Err(e)
                if e.chain()
                    .filter_map(|e| e.downcast_ref::<std::io::Error>())
                    .any(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                run("powershell.exe", &args, cwd, timeout)
            }
            value => value,
        }
    }
    #[cfg(unix)]
    {
        run("/bin/sh", &["-c", command], cwd, timeout)
    }
}

pub fn run(program: &str, args: &[&str], cwd: &Path, timeout: u64) -> Result<Value> {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in ["ZI_API_KEY", "OPENAI_API_KEY", "ANTHROPIC_API_KEY"] {
        command.env_remove(key);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let start = Instant::now();
    let child = command
        .spawn()
        .context("cannot start command (check installed program and PATH)")?;
    let mut guard = Guard::new(child)?;
    ACTIVE.store(guard.child.id(), Ordering::SeqCst);
    let stdout = capture(
        guard
            .child
            .stdout
            .take()
            .context("missing command stdout")?,
    );
    let stderr = capture(
        guard
            .child
            .stderr
            .take()
            .context("missing command stderr")?,
    );
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = guard.child.try_wait()? {
            break status;
        }
        if start.elapsed() >= Duration::from_secs(timeout) {
            timed_out = true;
            guard.terminate();
            break guard.child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    guard.terminate(); // Also close pipes held by background descendants.
    let (out, out_truncated) = stdout
        .join()
        .map_err(|_| anyhow::anyhow!("stdout reader failed"))??;
    let (err, err_truncated) = stderr
        .join()
        .map_err(|_| anyhow::anyhow!("stderr reader failed"))??;
    Ok(
        json!({"success": status.success() && !timed_out, "exit_code":status.code(), "timed_out":timed_out,
        "stdout":String::from_utf8_lossy(&out), "stderr":String::from_utf8_lossy(&err),
        "stdout_truncated":out_truncated, "stderr_truncated":err_truncated, "elapsed_ms":start.elapsed().as_millis()}),
    )
}

fn capture(
    mut source: impl Read + Send + 'static,
) -> std::thread::JoinHandle<std::io::Result<(Vec<u8>, bool)>> {
    std::thread::spawn(move || {
        let mut result = Vec::new();
        let mut truncated = false;
        let mut buffer = [0; 8192];
        loop {
            let n = source.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            let keep = n.min(OUTPUT_LIMIT.saturating_sub(result.len()));
            result.extend_from_slice(&buffer[..keep]);
            truncated |= keep < n;
        }
        Ok((result, truncated))
    })
}

struct Guard {
    child: Child,
    terminated: bool,
    #[cfg(windows)]
    job: windows_sys::Win32::Foundation::HANDLE,
}

impl Guard {
    fn new(mut child: Child) -> Result<Self> {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::*};
            // The handle is owned by Guard; KILL_ON_JOB_CLOSE covers descendants.
            let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if job.is_null() {
                let _ = child.kill();
                let _ = child.wait();
                bail!("cannot create command process job");
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = unsafe {
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    std::mem::size_of_val(&info) as u32,
                ) != 0
                    && AssignProcessToJobObject(job, child.as_raw_handle()) != 0
            };
            if !ok {
                unsafe {
                    CloseHandle(job);
                }
                let _ = child.kill();
                let _ = child.wait();
                bail!("cannot contain command process tree");
            }
            Ok(Self {
                child,
                job,
                terminated: false,
            })
        }
        #[cfg(unix)]
        {
            let _ = &mut child;
            Ok(Self {
                child,
                terminated: false,
            })
        }
    }

    fn terminate(&mut self) {
        if self.terminated {
            return;
        }
        self.terminated = true;
        #[cfg(unix)]
        {
            unsafe {
                libc::kill(-(self.child.id() as i32), libc::SIGKILL);
            }
        }
        #[cfg(windows)]
        {
            if !self.job.is_null() {
                unsafe {
                    windows_sys::Win32::Foundation::CloseHandle(self.job);
                }
                self.job = std::ptr::null_mut();
            }
        }
        let _ = self.child.kill();
        ACTIVE.store(0, Ordering::SeqCst);
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        self.terminate();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn command_captures_bounded_streams_and_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let command =
            "[Console]::Out.Write('x'*100000); [Console]::Error.Write('y'*100000); exit 7";
        #[cfg(unix)]
        let command = "printf '%0100000d' 0; printf '%0100000d' 0 >&2; exit 7";
        let result = shell(command, dir.path(), 15).unwrap();
        assert_eq!(result["exit_code"], 7);
        assert_eq!(result["success"], false);
        assert_eq!(result["stdout"].as_str().unwrap().len(), OUTPUT_LIMIT);
        assert_eq!(result["stderr"].as_str().unwrap().len(), OUTPUT_LIMIT);
        assert_eq!(result["stdout_truncated"], true);
        assert_eq!(result["stderr_truncated"], true);
    }
    #[test]
    fn timeout_cleans_up_child_process_tree() {
        let dir = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let command = "Write-Output ready; $child=Start-Process powershell.exe -WindowStyle Hidden -WorkingDirectory (Get-Location).Path -ArgumentList '-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 4; Set-Content escaped.txt escaped' -PassThru; $child.WaitForExit()";
        #[cfg(unix)]
        let command = "(sleep 4; printf escaped > escaped.txt) & printf ready; wait";
        let result = shell(command, dir.path(), 2).unwrap();
        assert_eq!(result["timed_out"], true);
        assert_eq!(result["success"], false);
        assert!(result["stdout"].as_str().unwrap().contains("ready"));
        std::thread::sleep(Duration::from_millis(4300));
        assert!(
            !dir.path().join("escaped.txt").exists(),
            "background descendant escaped timeout cleanup"
        );
    }
}
