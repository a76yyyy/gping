//! Pinger
//! This crate exposes a simple function to ping remote hosts across different operating systems.
//!
//! # Synchronous Example:
//! ```no_run
//! use std::time::Duration;
//! use pinger::{ping, PingResult, PingOptions};
//! let options = PingOptions::new("tomforb.es".to_string(), Duration::from_secs(1), None);
//! let stream = ping(options).expect("Error pinging");
//! for message in stream {
//!     match message {
//!         PingResult::Pong(duration, line) => println!("{:?} (line: {})", duration, line),
//!         PingResult::Timeout(_) => println!("Timeout!"),
//!         PingResult::Unknown(line) => println!("Unknown line: {}", line),
//!         PingResult::PingExited(_code, _stderr) => {}
//!     }
//! }
//! ```
//!
//! # Asynchronous Example (requires `async` feature):
//! ```no_run
//! # #[cfg(feature = "async")]
//! # async fn example() {
//! use std::time::Duration;
//! use pinger::{ping_async, PingResult, PingOptions};
//! let options = PingOptions::new("tomforb.es".to_string(), Duration::from_secs(1), None);
//! let mut stream = ping_async(options).await.expect("Error pinging");
//! while let Some(message) = stream.recv().await {
//!     match message {
//!         PingResult::Pong(duration, line) => println!("{:?} (line: {})", duration, line),
//!         PingResult::Timeout(_) => println!("Timeout!"),
//!         PingResult::Unknown(line) => println!("Unknown line: {}", line),
//!         PingResult::PingExited(_code, _stderr) => {}
//!     }
//! }
//! # }
//! ```

use lazy_regex::Regex;
use std::ffi::OsStr;
use std::fmt::{Debug, Formatter};
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{mpsc, Arc};
use std::time::Duration;
use std::{fmt, io, thread};
use target::Target;
use thiserror::Error;

#[cfg(feature = "async")]
use async_trait::async_trait;

#[cfg(unix)]
pub mod linux;
#[cfg(unix)]
pub mod macos;
#[cfg(windows)]
pub mod windows;

#[cfg(unix)]
mod bsd;
#[cfg(feature = "fake-ping")]
mod fake;
pub mod target;
#[cfg(test)]
mod test;
pub mod utils;

#[derive(Debug, Clone)]
pub struct PingOptions {
    pub target: Target,
    pub interval: Duration,
    pub interface: Option<String>,
    pub raw_arguments: Option<Vec<String>>,
}

impl PingOptions {
    pub fn with_raw_arguments(mut self, raw_arguments: Vec<impl ToString>) -> Self {
        self.raw_arguments = Some(
            raw_arguments
                .into_iter()
                .map(|item| item.to_string())
                .collect(),
        );
        self
    }
}

impl PingOptions {
    pub fn from_target(target: Target, interval: Duration, interface: Option<String>) -> Self {
        Self {
            target,
            interval,
            interface,
            raw_arguments: None,
        }
    }
    pub fn new(target: impl ToString, interval: Duration, interface: Option<String>) -> Self {
        Self::from_target(Target::new_any(target), interval, interface)
    }

    pub fn new_ipv4(target: impl ToString, interval: Duration, interface: Option<String>) -> Self {
        Self::from_target(Target::new_ipv4(target), interval, interface)
    }

    pub fn new_ipv6(target: impl ToString, interval: Duration, interface: Option<String>) -> Self {
        Self::from_target(Target::new_ipv6(target), interval, interface)
    }
}

pub fn run_ping(
    cmd: impl AsRef<OsStr> + Debug,
    args: Vec<impl AsRef<OsStr> + Debug>,
) -> Result<Child, PingCreationError> {
    Ok(Command::new(cmd.as_ref())
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Required to ensure that the output is formatted in the way we expect, not
        // using locale specific delimiters.
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .spawn()?)
}

#[cfg(feature = "async")]
pub async fn run_ping_async(
    cmd: impl AsRef<OsStr> + Debug,
    args: Vec<impl AsRef<OsStr> + Debug>,
) -> Result<tokio::process::Child, PingCreationError> {
    Ok(tokio::process::Command::new(cmd.as_ref())
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Required to ensure that the output is formatted in the way we expect, not
        // using locale specific delimiters.
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .kill_on_drop(true) // Ensure child process is killed when Child is dropped
        .spawn()?)
}

pub(crate) fn extract_regex(regex: &Regex, line: String) -> Option<PingResult> {
    let cap = regex.captures(&line)?;
    let ms = cap
        .name("ms")
        .expect("No capture group named 'ms'")
        .as_str()
        .parse::<u64>()
        .ok()?;
    let ns = match cap.name("ns") {
        None => 0,
        Some(cap) => {
            let matched_str = cap.as_str();
            let number_of_digits = matched_str.len() as u32;
            let fractional_ms = matched_str.parse::<u64>().ok()?;
            fractional_ms * (10u64.pow(6 - number_of_digits))
        }
    };
    let duration = Duration::from_millis(ms) + Duration::from_nanos(ns);
    Some(PingResult::Pong(duration, line))
}

pub trait Pinger: Send + Sync {
    fn from_options(options: PingOptions) -> std::result::Result<Self, PingCreationError>
    where
        Self: Sized;

    fn parse_fn(&self) -> fn(String) -> Option<PingResult>;

    fn ping_args(&self) -> (&str, Vec<String>);

    fn start(&self) -> Result<mpsc::Receiver<PingResult>, PingCreationError> {
        let (tx, rx) = mpsc::channel();
        let (cmd, args) = self.ping_args();

        let mut child = run_ping(cmd, args)?;
        let stdout = child.stdout.take().expect("child did not have a stdout");

        let parse_fn = self.parse_fn();

        thread::spawn(move || {
            let reader = BufReader::new(stdout).lines();
            for line in reader {
                match line {
                    Ok(msg) => {
                        if let Some(result) = parse_fn(msg) {
                            if tx.send(result).is_err() {
                                break;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            let result = child.wait_with_output().expect("Child wasn't started?");
            let decoded_stderr = String::from_utf8(result.stderr).expect("Error decoding stderr");
            let _ = tx.send(PingResult::PingExited(result.status, decoded_stderr));
        });

        Ok(rx)
    }
}

#[cfg(feature = "async")]
#[async_trait]
pub trait AsyncPinger: Send + Sync {
    async fn from_options(options: PingOptions) -> std::result::Result<Self, PingCreationError>
    where
        Self: Sized;

    fn parse_fn(&self) -> fn(String) -> Option<PingResult>;

    fn ping_args(&self) -> (&str, Vec<String>);

    async fn start(
        &self,
    ) -> Result<tokio::sync::mpsc::UnboundedReceiver<PingResult>, PingCreationError> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let (cmd, args) = self.ping_args();

        let mut child = run_ping_async(cmd, args).await?;
        let stdout = child.stdout.take().expect("child did not have a stdout");
        let stderr = child.stderr.take().expect("child did not have a stderr");

        let parse_fn = self.parse_fn();

        tokio::spawn(async move {
            use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            // Read stderr in background task to avoid pipe blocking
            let stderr_task = tokio::spawn(async move {
                let mut stderr_reader = BufReader::new(stderr);
                let mut stderr_content = String::new();
                let _ = stderr_reader.read_to_string(&mut stderr_content).await;
                stderr_content
            });

            // Read output
            while let Ok(Some(msg)) = lines.next_line().await {
                if let Some(result) = parse_fn(msg) {
                    if tx.send(result).is_err() {
                        // Receiver closed, terminate process
                        let _ = child.kill().await;
                        return;
                    }
                }
            }

            // Normal completion, wait for process exit (with timeout)
            tokio::select! {
                result = child.wait() => {
                    // Get stderr content
                    let stderr_content = stderr_task.await.unwrap_or_default();

                    match result {
                        Ok(status) => {
                            // Process exited normally
                            let _ = tx.send(PingResult::PingExited(status, stderr_content));
                        }
                        Err(e) => {
                            // wait failed
                            let _ = tx.send(PingResult::PingExited(
                                ExitStatus::default(),
                                format!("Failed to wait for child: {e}. Stderr: {stderr_content}"),
                            ));
                        }
                    }
                }
                () = tokio::time::sleep(Duration::from_secs(5)) => {
                    // Timeout, force kill process
                    let _ = child.kill().await;

                    // Try to get stderr (may be incomplete)
                    let stderr_content = stderr_task.await.unwrap_or_else(|_| "stderr unavailable".to_string());

                    let _ = tx.send(PingResult::PingExited(
                        ExitStatus::default(),
                        format!("Process killed after timeout. Stderr: {stderr_content}"),
                    ));
                }
            }
        });

        Ok(rx)
    }
}

#[derive(Debug)]
pub enum PingResult {
    Pong(Duration, String),
    Timeout(String),
    Unknown(String),
    PingExited(ExitStatus, String),
}

impl fmt::Display for PingResult {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self {
            PingResult::Pong(duration, _) => write!(f, "{duration:?}"),
            PingResult::Timeout(_) => write!(f, "Timeout"),
            PingResult::Unknown(_) => write!(f, "Unknown"),
            PingResult::PingExited(status, stderr) => write!(f, "Exited({status}, {stderr})"),
        }
    }
}

#[derive(Error, Debug)]
pub enum PingCreationError {
    #[error("Could not detect ping. Stderr: {stderr:?}\nStdout: {stdout:?}")]
    UnknownPing {
        stderr: Vec<String>,
        stdout: Vec<String>,
    },
    #[error("Error spawning ping: {0}")]
    SpawnError(#[from] io::Error),

    #[error("Installed ping is not supported: {alternative}")]
    NotSupported { alternative: String },

    #[error("Invalid or unresolvable hostname {0}")]
    HostnameError(String),
}

pub fn get_pinger(options: PingOptions) -> std::result::Result<Arc<dyn Pinger>, PingCreationError> {
    #[cfg(feature = "fake-ping")]
    if std::env::var("PINGER_FAKE_PING")
        .map(|e| e == "1")
        .unwrap_or_default()
    {
        return Ok(Arc::new(fake::FakePinger::from_options(options)?));
    }

    #[cfg(windows)]
    {
        return Ok(Arc::new(windows::WindowsPinger::from_options(options)?));
    }
    #[cfg(unix)]
    {
        if cfg!(target_os = "freebsd")
            || cfg!(target_os = "dragonfly")
            || cfg!(target_os = "openbsd")
            || cfg!(target_os = "netbsd")
        {
            Ok(Arc::new(bsd::BSDPinger::from_options(options)?))
        } else if cfg!(target_os = "macos") {
            Ok(Arc::new(macos::MacOSPinger::from_options(options)?))
        } else {
            Ok(Arc::new(linux::LinuxPinger::from_options(options)?))
        }
    }
}

/// Start pinging an address. The address can be either a hostname or an IP address.
pub fn ping(
    options: PingOptions,
) -> std::result::Result<mpsc::Receiver<PingResult>, PingCreationError> {
    let pinger = get_pinger(options)?;
    pinger.start()
}

#[cfg(feature = "async")]
pub async fn get_async_pinger(
    options: PingOptions,
) -> std::result::Result<Arc<dyn AsyncPinger>, PingCreationError> {
    #[cfg(feature = "fake-ping")]
    if std::env::var("PINGER_FAKE_PING")
        .map(|e| e == "1")
        .unwrap_or_default()
    {
        return Ok(Arc::new(
            fake::FakeAsyncPinger::from_options(options).await?,
        ));
    }

    #[cfg(windows)]
    {
        return Ok(Arc::new(
            windows::WindowsAsyncPinger::from_options(options).await?,
        ));
    }
    #[cfg(unix)]
    {
        if cfg!(target_os = "freebsd")
            || cfg!(target_os = "dragonfly")
            || cfg!(target_os = "openbsd")
            || cfg!(target_os = "netbsd")
        {
            Ok(Arc::new(bsd::BSDAsyncPinger::from_options(options).await?))
        } else if cfg!(target_os = "macos") {
            Ok(Arc::new(
                macos::MacOSAsyncPinger::from_options(options).await?,
            ))
        } else {
            Ok(Arc::new(
                linux::LinuxAsyncPinger::from_options(options).await?,
            ))
        }
    }
}

/// Start pinging an address asynchronously. The address can be either a hostname or an IP address.
/// Requires the `async` feature to be enabled.
#[cfg(feature = "async")]
pub async fn ping_async(
    options: PingOptions,
) -> std::result::Result<tokio::sync::mpsc::UnboundedReceiver<PingResult>, PingCreationError> {
    let pinger = get_async_pinger(options).await?;
    pinger.start().await
}
