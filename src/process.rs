use async_channel::Receiver;
use colored::Colorize;
use log::error;
#[cfg(not(target_os = "windows"))]
pub use nix::sys::signal::Signal;
#[cfg(not(target_os = "windows"))]
use nix::{sys::signal, unistd};
use std::collections::HashMap;
#[cfg(not(target_os = "windows"))]
use std::collections::HashSet;
use std::ffi::OsStr;
use std::io;
use std::process::Stdio;
use std::time::Duration;
#[cfg(not(target_os = "windows"))]
use std::time::Instant;
#[cfg(not(target_os = "windows"))]
use sysinfo::{Pid, PidExt, ProcessExt, System, SystemExt};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::Command;
use tokio::task;
use tokio::time::sleep;

#[cfg(target_os = "windows")]
use std::ptr::null_mut;
#[cfg(target_os = "windows")]
use winapi::um::processthreadsapi::{OpenProcess, TerminateProcess};
#[cfg(target_os = "windows")]
use winapi::um::winnt::{PROCESS_QUERY_INFORMATION, PROCESS_TERMINATE};

pub const SLEEP_STEP: Duration = Duration::from_millis(100);

#[cfg(target_os = "windows")]
fn kill(pid: u32) {
    let pc = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_TERMINATE, 0, pid) };
    if pc != null_mut() {
        unsafe { TerminateProcess(pc, 1) };
    }
}

pub fn suicide(timeout: Duration, warn: bool) {
    if warn {
        let msg = format!("Killing process in {:?}", timeout);
        eprintln!("{}", msg.red().bold());
    }
    std::thread::spawn(move || {
        let pid = std::process::id();
        std::thread::sleep(timeout);
        #[allow(clippy::cast_possible_wrap)]
        #[cfg(not(target_os = "windows"))]
        let _ = signal::kill(unistd::Pid::from_raw(pid as i32), Signal::SIGKILL);
        #[cfg(target_os = "windows")]
        kill(pid);
    });
}

#[derive(Debug)]
pub struct CommandResult {
    pub code: Option<i32>,
    pub out: Vec<String>,
    pub err: Vec<String>,
}

impl Default for CommandResult {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandResult {
    #[must_use]
    pub fn new() -> Self {
        Self {
            code: None,
            out: Vec::new(),
            err: Vec::new(),
        }
    }

    #[must_use]
    pub fn ok(&self) -> bool {
        match self.code {
            Some(v) => v == 0,
            None => false,
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn get_child_pids_recursive(pid: Pid, sys: &System, to: &mut HashSet<Pid>) {
    for (i, p) in sys.processes() {
        if let Some(parent) = p.parent() {
            if parent == pid {
                to.insert(*i);
                get_child_pids_recursive(*i, sys, to);
            }
        };
    }
}

#[cfg(not(target_os = "windows"))]
pub fn kill_pstree_with_signal(pid: u32, signal: Signal, kill_parent: bool) {
    let mut sys = System::new();
    let mut pids = HashSet::new();
    kill_pstree_with_signal_impl(Pid::from_u32(pid), &mut sys, &mut pids, signal, kill_parent);
}

#[allow(clippy::cast_possible_wrap)]
#[cfg(not(target_os = "windows"))]
fn kill_pstree_with_signal_impl(
    pid: Pid,
    sys: &mut sysinfo::System,
    pids: &mut HashSet<Pid>,
    signal: Signal,
    kill_parent: bool,
) {
    sys.refresh_processes();
    if kill_parent {
        pids.insert(pid);
    }
    get_child_pids_recursive(pid, sys, pids);
    for cpid in pids.iter() {
        let _ = signal::kill(unistd::Pid::from_raw(cpid.as_u32() as i32), signal);
    }
}

#[cfg(not(target_os = "windows"))]
pub fn kill_pstree_sync(pid: u32, kill_parent: bool) {
    let mut sys = System::new();
    let mut pids = HashSet::new();
    kill_pstree_with_signal_impl(
        Pid::from_u32(pid),
        &mut sys,
        &mut pids,
        Signal::SIGKILL,
        kill_parent,
    );
}

#[allow(clippy::cast_possible_wrap)]
#[cfg(not(target_os = "windows"))]
pub async fn kill_pstree(pid: u32, tki: Option<Duration>, kill_parent: bool) {
    let mut sys = System::new();
    let mut pids = HashSet::new();
    let signal = if tki.is_some() {
        Signal::SIGTERM
    } else {
        Signal::SIGKILL
    };
    let pid = Pid::from_u32(pid);
    kill_pstree_with_signal_impl(pid, &mut sys, &mut pids, signal, kill_parent);
    if !pids.is_empty() || kill_parent {
        if let Some(t) = tki {
            let now = Instant::now();
            while now.elapsed() < t {
                sleep(SLEEP_STEP).await;
                if signal::kill(unistd::Pid::from_raw(pid.as_u32() as i32), Signal::SIGTERM)
                    .is_err()
                {
                    break;
                }
            }
            kill_pstree_with_signal_impl(pid, &mut sys, &mut pids, Signal::SIGKILL, kill_parent);
        }
    }
}

#[derive(Debug)]
enum CommandFrame {
    Finished(i32),
    Terminated,
    Stdout(String),
    Stderr(String),
    Error(io::Error),
}

#[derive(Default, Clone)]
pub struct Options<'a> {
    environment: HashMap<&'a str, &'a str>,
    tki: Option<Duration>,
    input_data: Option<std::borrow::Cow<'a, Vec<u8>>>,
}

impl<'a> Options<'a> {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }
    #[inline]
    pub fn tki(mut self, t: Duration) -> Self {
        self.tki.replace(t);
        self
    }
    #[inline]
    pub fn input(mut self, data: std::borrow::Cow<'a, Vec<u8>>) -> Self {
        self.input_data.replace(data);
        self
    }
    #[inline]
    pub fn env(mut self, name: &'a str, value: &'a str) -> Self {
        self.environment.insert(name, value);
        self
    }
    #[inline]
    pub fn environment(&self) -> &HashMap<&str, &str> {
        &self.environment
    }
    #[inline]
    pub fn environment_mut(&'a mut self) -> &mut HashMap<&str, &str> {
        &mut self.environment
    }
}

#[allow(clippy::too_many_lines)]
#[allow(clippy::missing_panics_doc)]
/// # Errors
///
/// Will return `Err` on I/O errors
pub async fn command<P, I, S>(
    program: P,
    args: I,
    timeout: Duration,
    opts: Options<'_>,
) -> Result<CommandResult, io::Error>
where
    P: AsRef<OsStr>,
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut child = Command::new(program)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .args(args)
        .envs(opts.environment)
        .spawn()?;
    let stdin = if opts.input_data.is_some() {
        match child.stdin.take() {
            Some(v) => Some(v),
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "Unable to create stdin writer",
                ))
            }
        }
    } else {
        None
    };
    let stdin_writer = stdin.map(BufWriter::new);
    let Some(stdout) = child.stdout.take() else {
        return Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "Unable to create stdout reader",
        ));
    };
    let mut stdout_reader = BufReader::new(stdout).lines();
    let Some(stderr) = child.stderr.take() else {
        return Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "Unable to create stderr reader",
        ));
    };
    let mut stderr_reader = BufReader::new(stderr).lines();
    let ppid = child.id();
    let (tx_runner, rx) = async_channel::bounded(2);
    let tx_guard = tx_runner.clone();
    let tx_out = tx_runner.clone();
    let tx_err = tx_runner.clone();
    let runner = task::spawn(async move {
        let frame = match child.wait().await {
            Ok(v) => CommandFrame::Finished(if let Some(v) = v.code() {
                v
            } else {
                // killed, wait guard to finish
                sleep(timeout).await;
                -15
            }),
            Err(e) => CommandFrame::Error(e),
        };
        let _r = tx_runner.send(frame).await;
    });
    let guard = ppid.map(|pid| {
        task::spawn(async move {
            sleep(timeout).await;
            #[allow(clippy::cast_possible_wrap)]
            #[cfg(not(target_os = "windows"))]
            kill_pstree(pid, opts.tki, true).await;
            #[cfg(target_os = "windows")]
            kill(pid);
            let _r = tx_guard.send(CommandFrame::Terminated).await;
        })
    });
    let fut_stdin = stdin_writer.map(|mut writer| {
        let input_data = opts.input_data.unwrap().into_owned();
        task::spawn(async move {
            if let Err(e) = writer.write_all(&input_data).await {
                error!("Unable to write to stdin: {}", e);
            } else if let Err(e) = writer.flush().await {
                error!("Unable to flush stdin: {}", e);
            }
        })
    });
    let fut_stdout = task::spawn(async move {
        while let Some(line) = match stdout_reader.next_line().await {
            Ok(v) => v,
            Err(e) => {
                let _r = tx_out.send(CommandFrame::Error(e)).await;
                return;
            }
        } {
            let _r = tx_out.send(CommandFrame::Stdout(line)).await;
        }
    });
    let fut_stderr = task::spawn(async move {
        while let Some(line) = match stderr_reader.next_line().await {
            Ok(v) => v,
            Err(e) => {
                let _r = tx_err.send(CommandFrame::Error(e)).await;
                return;
            }
        } {
            let _r = tx_err.send(CommandFrame::Stderr(line)).await;
        }
    });
    let mut result = CommandResult::new();
    while let Ok(r) = rx.recv().await {
        match r {
            CommandFrame::Finished(code) => {
                if let Some(g) = guard {
                    g.abort();
                }
                result.code = Some(code);
                // finish reading stdout / stderr
                while let Ok(r) = rx.recv().await {
                    match r {
                        CommandFrame::Stdout(v) => result.out.push(v),
                        CommandFrame::Stderr(v) => result.err.push(v),
                        _ => {}
                    }
                }
                return Ok(result);
            }
            CommandFrame::Terminated => {
                runner.abort();
                if let Some(f) = fut_stdin {
                    f.abort();
                }
                fut_stdout.abort();
                fut_stderr.abort();
                return Ok(result);
            }
            CommandFrame::Error(e) => {
                runner.abort();
                if let Some(g) = guard {
                    g.abort();
                }
                if let Some(f) = fut_stdin {
                    f.abort();
                }
                fut_stdout.abort();
                fut_stderr.abort();
                #[allow(clippy::cast_possible_wrap)]
                ppid.map(|pid| async move {
                    #[cfg(not(target_os = "windows"))]
                    kill_pstree(pid, opts.tki, true).await;
                    #[cfg(target_os = "windows")]
                    kill(pid);
                });
                return Err(e);
            }
            CommandFrame::Stdout(v) => result.out.push(v),
            CommandFrame::Stderr(v) => result.err.push(v),
        }
    }
    Ok(result)
}

#[derive(Debug)]
pub enum CommandPipeOutput {
    Stdout(String),
    Stderr(String),
    Terminated(i32),
}

/// # Panics
///
/// Should not panic
pub fn command_pipe<P, I, S>(
    program: P,
    args: I,
    opts: Options<'_>,
) -> Result<Receiver<CommandPipeOutput>, io::Error>
where
    P: AsRef<OsStr>,
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let (output_tx, output_rx) = async_channel::bounded(512);

    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .envs(opts.environment())
        .spawn()?;
    let stdin = if opts.input_data.is_some() {
        match child.stdin.take() {
            Some(v) => Some(v),
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "Unable to create stdin writer",
                ))
            }
        }
    } else {
        None
    };
    let stdin_writer = stdin.map(BufWriter::new);
    let stderr = child.stderr.take().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::BrokenPipe,
            "Failed to capture stderr of child process",
        )
    })?;
    let stdout = child.stdout.take().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::BrokenPipe,
            "Failed to capture stdout of child process",
        )
    })?;
    let fut_stdin = stdin_writer.map(|mut writer| {
        let input_data = opts.input_data.unwrap().into_owned();
        task::spawn(async move {
            if let Err(e) = writer.write_all(&input_data).await {
                error!("Unable to write to stdin: {}", e);
            } else if let Err(e) = writer.flush().await {
                error!("Unable to flush stdin: {}", e);
            }
        })
    });

    tokio::spawn(async move {
        let output_tx_stderr = output_tx.clone();

        let stderr_handle = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            while reader.read_line(&mut line).await.is_ok() {
                if line.is_empty()
                    || (output_tx_stderr
                        .send(CommandPipeOutput::Stderr(line.clone()))
                        .await)
                        .is_err()
                {
                    break;
                }
                line.clear();
            }
        });

        let output_tx_stdout = output_tx.clone();

        let stdout_handle = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            while reader.read_line(&mut line).await.is_ok() {
                if line.is_empty()
                    || (output_tx_stdout
                        .send(CommandPipeOutput::Stdout(line.clone()))
                        .await)
                        .is_err()
                {
                    break;
                }
                line.clear();
            }
        });

        let mut exit_code = -99;
        if let Ok(x) = child.wait().await {
            if let Some(code) = x.code() {
                exit_code = code;
            }
        }
        if let Some(v) = fut_stdin {
            v.abort();
        }
        tokio::select!(
            _ = stderr_handle => {},
            _ = stdout_handle => {},
        );
        let _ = output_tx
            .send(CommandPipeOutput::Terminated(exit_code))
            .await;
    });

    Ok(output_rx)
}
