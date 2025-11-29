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

/// Configuration options for ping operations
#[derive(Debug, Clone)]
pub struct PingOptions {
    /// Target address to ping
    pub target: Target,
    /// Interval between ping requests
    pub interval: Duration,
    /// Network interface to use (optional)
    pub interface: Option<String>,
    /// Additional raw arguments to pass to the ping command (optional)
    pub raw_arguments: Option<Vec<String>>,
}

impl PingOptions {
    /// Add raw arguments to pass to the ping command
    ///
    /// These arguments will be passed directly to the underlying ping command.
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
    /// Create new ping options from a target
    pub fn from_target(target: Target, interval: Duration, interface: Option<String>) -> Self {
        Self {
            target,
            interval,
            interface,
            raw_arguments: None,
        }
    }

    /// Create new ping options with any IP version
    ///
    /// The target can be an IP address or hostname that resolves to either IPv4 or IPv6.
    pub fn new(target: impl Into<String>, interval: Duration, interface: Option<String>) -> Self {
        Self::from_target(Target::new_any(target), interval, interface)
    }

    /// Create new ping options constrained to IPv4
    ///
    /// The target can be an IPv4 address or hostname that must resolve to IPv4.
    pub fn new_ipv4(
        target: impl Into<String>,
        interval: Duration,
        interface: Option<String>,
    ) -> Self {
        Self::from_target(Target::new_ipv4(target), interval, interface)
    }

    /// Create new ping options constrained to IPv6
    ///
    /// The target can be an IPv6 address or hostname that must resolve to IPv6.
    pub fn new_ipv6(
        target: impl Into<String>,
        interval: Duration,
        interface: Option<String>,
    ) -> Self {
        Self::from_target(Target::new_ipv6(target), interval, interface)
    }
}

/// Run a ping command synchronously
///
/// # Errors
///
/// - [`PingCreationError::SpawnError`] - The command fails to spawn.
///
/// # Note
///
/// The `Debug` trait bound is kept for debugging purposes, even though it's not used in the function body.
/// This allows callers to debug-print the arguments if needed.
pub fn run_ping(
    cmd: impl AsRef<OsStr> + Debug,
    args: impl IntoIterator<Item = impl AsRef<OsStr> + Debug>,
) -> Result<Child, PingCreationError> {
    Ok(Command::new(cmd.as_ref())
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Required to ensure that the output is formatted in the way we expect, not
        // using locale specific delimiters.
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .spawn()?)
}

/// Run a ping command asynchronously
///
/// # Errors
///
/// - [`PingCreationError::SpawnError`] - The command fails to spawn.
///
/// # Note
///
/// The `Debug` trait bound is kept for debugging purposes, even though it's not used in the function body.
/// This allows callers to debug-print the arguments if needed.
#[cfg(feature = "async")]
pub async fn run_ping_async(
    cmd: impl AsRef<OsStr> + Debug,
    args: impl IntoIterator<Item = impl AsRef<OsStr> + Debug>,
) -> Result<tokio::process::Child, PingCreationError> {
    Ok(tokio::process::Command::new(cmd.as_ref())
        .args(args)
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
    let ms = cap.name("ms")?.as_str().parse::<u64>().ok()?;
    let ns = match cap.name("ns") {
        None => 0,
        Some(cap) => {
            let matched_str = cap.as_str();
            let number_of_digits = u32::try_from(matched_str.len().min(6)).unwrap_or(6);
            let fractional_ms = matched_str.parse::<u64>().ok()?;
            fractional_ms * (10u64.pow(6 - number_of_digits))
        }
    };
    let duration = Duration::from_millis(ms) + Duration::from_nanos(ns);
    Some(PingResult::Pong(duration, line))
}

/// Trait for platform-specific ping implementations
pub trait Pinger: Send + Sync {
    /// Create a new pinger from options
    ///
    /// # Errors
    ///
    /// - [`PingCreationError::UnknownPing`] - The ping command cannot be detected
    /// - [`PingCreationError::NotSupported`] - The ping command is not supported
    /// - [`PingCreationError::SpawnError`] - The command fails to spawn
    fn from_options(options: PingOptions) -> std::result::Result<Self, PingCreationError>
    where
        Self: Sized;

    /// Get the parser function for this platform's ping output
    fn parse_fn(&self) -> fn(String) -> Option<PingResult>;

    /// Get the command and arguments for this platform's ping
    fn ping_args(&self) -> (&str, Vec<String>);

    /// Start the ping process and return a receiver for results
    ///
    /// # Errors
    ///
    /// - [`PingCreationError::SpawnError`] - The ping process fails to start or stdout cannot be captured
    fn start(&self) -> Result<mpsc::Receiver<PingResult>, PingCreationError> {
        let (tx, rx) = mpsc::channel();
        let (cmd, args) = self.ping_args();

        let mut child = run_ping(cmd, args)?;
        let stdout = child.stdout.take().ok_or_else(|| {
            PingCreationError::SpawnError(std::io::Error::other("child did not have a stdout"))
        })?;

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
            let Ok(result) = child.wait_with_output() else {
                return;
            };
            let decoded_stderr =
                String::from_utf8(result.stderr).unwrap_or_else(|_| "<invalid UTF-8>".to_string());
            let _ = tx.send(PingResult::PingExited(result.status, decoded_stderr));
        });

        Ok(rx)
    }
}

/// Trait for platform-specific asynchronous ping implementations
#[cfg(feature = "async")]
#[async_trait]
pub trait AsyncPinger: Send + Sync {
    /// Create a new async pinger from options
    ///
    /// # Errors
    ///
    /// - [`PingCreationError::UnknownPing`] - The ping command cannot be detected
    /// - [`PingCreationError::NotSupported`] - The ping command is not supported
    /// - [`PingCreationError::SpawnError`] - The command fails to spawn
    async fn from_options(options: PingOptions) -> std::result::Result<Self, PingCreationError>
    where
        Self: Sized;

    /// Get the parser function for this platform's ping output
    fn parse_fn(&self) -> fn(String) -> Option<PingResult>;

    /// Get the command and arguments for this platform's ping
    fn ping_args(&self) -> (&str, Vec<String>);

    /// Start the ping process asynchronously and return a receiver for results
    ///
    /// # Errors
    ///
    /// - [`PingCreationError::SpawnError`] - The ping process fails to start or stdout/stderr cannot be captured
    async fn start(
        &self,
    ) -> Result<tokio::sync::mpsc::UnboundedReceiver<PingResult>, PingCreationError> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let (cmd, args) = self.ping_args();

        let mut child = run_ping_async(cmd, args).await?;
        let stdout = child.stdout.take().ok_or_else(|| {
            PingCreationError::SpawnError(std::io::Error::other("child did not have a stdout"))
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            PingCreationError::SpawnError(std::io::Error::other("child did not have a stderr"))
        })?;

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

/// Result of a ping operation
#[derive(Debug)]
pub enum PingResult {
    /// Successful ping response with round-trip time and raw output line
    Pong(Duration, String),
    /// Ping timeout with raw output line
    Timeout(String),
    /// Unknown ping output line that couldn't be parsed
    Unknown(String),
    /// Ping process exited with status and stderr output
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

/// Errors that can occur when creating a ping process
#[derive(Error, Debug)]
pub enum PingCreationError {
    /// Could not detect the ping command version
    #[error("Could not detect ping. Stderr: {stderr:?}\nStdout: {stdout:?}")]
    UnknownPing {
        /// Standard error output
        stderr: Vec<String>,
        /// Standard output
        stdout: Vec<String>,
    },
    /// Error spawning the ping process
    #[error("Error spawning ping: {0}")]
    SpawnError(#[from] io::Error),

    /// The installed ping command is not supported
    #[error("Installed ping is not supported: {alternative}")]
    NotSupported {
        /// Alternative suggestion
        alternative: String,
    },

    /// Invalid or unresolvable hostname
    #[error("Invalid or unresolvable hostname {0}")]
    HostnameError(String),
}

/// Get a platform-specific pinger implementation
///
/// # Errors
///
/// - [`PingCreationError::UnknownPing`] - The ping command cannot be detected
/// - [`PingCreationError::NotSupported`] - The ping command is not supported
/// - [`PingCreationError::SpawnError`] - The command fails to spawn
pub fn get_pinger(options: PingOptions) -> std::result::Result<Arc<dyn Pinger>, PingCreationError> {
    #[cfg(feature = "fake-ping")]
    if std::env::var("PINGER_FAKE_PING").is_ok_and(|e| e == "1") {
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
///
/// # Errors
///
/// Returns [`PingCreationError`] if the pinger cannot be created or started.
/// See [`get_pinger`] for possible error types.
pub fn ping(
    options: PingOptions,
) -> std::result::Result<mpsc::Receiver<PingResult>, PingCreationError> {
    let pinger = get_pinger(options)?;
    pinger.start()
}

/// Get a platform-specific async pinger implementation
///
/// # Errors
///
/// - [`PingCreationError::UnknownPing`] - The ping command cannot be detected
/// - [`PingCreationError::NotSupported`] - The ping command is not supported
/// - [`PingCreationError::SpawnError`] - The command fails to spawn
#[cfg(feature = "async")]
pub async fn get_async_pinger(
    options: PingOptions,
) -> std::result::Result<Arc<dyn AsyncPinger>, PingCreationError> {
    #[cfg(feature = "fake-ping")]
    if std::env::var("PINGER_FAKE_PING").is_ok_and(|e| e == "1") {
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
///
/// # Errors
///
/// - [`PingCreationError::UnknownPing`] - The ping command cannot be detected
/// - [`PingCreationError::NotSupported`] - The ping command is not supported
/// - [`PingCreationError::SpawnError`] - The command fails to spawn or stdout/stderr cannot be captured
#[cfg(feature = "async")]
pub async fn ping_async(
    options: PingOptions,
) -> std::result::Result<tokio::sync::mpsc::UnboundedReceiver<PingResult>, PingCreationError> {
    let pinger = get_async_pinger(options).await?;
    pinger.start().await
}
