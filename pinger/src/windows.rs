use crate::utils::resolve_target;
use crate::PingCreationError;
use crate::{extract_regex, PingOptions, PingResult, Pinger};
use lazy_regex::*;
use std::net::IpAddr;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use winping::{Buffer, Error as WinPingError, Pinger as WinPinger};

#[cfg(feature = "async")]
use crate::AsyncPinger;
#[cfg(feature = "async")]
use async_trait::async_trait;
#[cfg(feature = "async")]
use winping::AsyncPinger as AsyncWinPinger;

pub static RE: Lazy<Regex> = lazy_regex!(r"(?ix-u)time=(?P<ms>\d+)(?:\.(?P<ns>\d+))?");

// =================== Helper Functions ===================

/// Create and configure synchronous WinPinger
fn create_pinger(interval: Duration) -> WinPinger {
    let mut pinger = WinPinger::new().expect("Failed to create a WinPinger instance");
    let timeout_ms = interval.as_millis() as u32;
    pinger.set_timeout(timeout_ms);
    pinger
}

/// Create and configure asynchronous AsyncWinPinger
#[cfg(feature = "async")]
fn create_async_pinger(interval: Duration) -> AsyncWinPinger {
    let mut pinger = AsyncWinPinger::new();
    let timeout_ms = interval.as_millis() as u32;
    pinger.set_timeout(timeout_ms);
    pinger
}

/// Handle ping errors, distinguish between timeout and other errors
fn handle_ping_error(
    error: WinPingError,
    target_ip: IpAddr,
    send_timeout: impl FnOnce() -> bool,
    send_error: impl FnOnce() -> bool,
) -> bool {
    match error {
        WinPingError::Timeout => {
            // Timeout - continue ping
            send_timeout()
        }
        _ => {
            // Other errors - send error and exit
            eprintln!("Ping error for {}: {:?}", target_ip, error);
            send_error();
            false
        }
    }
}

/// Calculate wait time for next ping, compensating for ping duration
fn calculate_wait_time(last_ping_time: Instant, interval: Duration) -> Duration {
    let elapsed = last_ping_time.elapsed();
    interval.saturating_sub(elapsed)
}

pub struct WindowsPinger {
    options: PingOptions,
}

impl Pinger for WindowsPinger {
    fn from_options(options: PingOptions) -> Result<Self, PingCreationError> {
        Ok(Self { options })
    }

    fn parse_fn(&self) -> fn(String) -> Option<PingResult> {
        |line| {
            if line.contains("timed out") || line.contains("failure") {
                return Some(PingResult::Timeout(line));
            }
            extract_regex(&RE, line)
        }
    }

    fn ping_args(&self) -> (&str, Vec<String>) {
        unimplemented!("ping_args for WindowsPinger is not implemented")
    }

    fn start(&self) -> Result<mpsc::Receiver<PingResult>, PingCreationError> {
        let interval = self.options.interval;

        // Resolve target IP address
        let parsed_ip = resolve_target(&self.options.target)?;

        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            // Create and configure pinger
            let pinger = create_pinger(interval);
            let mut buffer = Buffer::new();
            let mut last_ping_time = Instant::now();

            loop {
                // Send ping request
                match pinger.send(parsed_ip, &mut buffer) {
                    Ok(rtt) => {
                        let result = PingResult::Pong(
                            Duration::from_millis(rtt as u64),
                            format!("Reply from {}: time={}ms", parsed_ip, rtt),
                        );
                        if tx.send(result).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let should_continue = handle_ping_error(
                            e,
                            parsed_ip,
                            || {
                                tx.send(PingResult::Timeout(format!(
                                    "Request timeout for {}",
                                    parsed_ip
                                )))
                                .is_ok()
                            },
                            || {
                                let _ = tx.send(PingResult::PingExited(
                                    std::process::ExitStatus::default(),
                                    format!("Ping error: {:?}", e),
                                ));
                                true
                            },
                        );

                        if !should_continue {
                            break;
                        }
                    }
                }

                // Calculate wait time, compensating for ping duration
                let wait_time = calculate_wait_time(last_ping_time, interval);
                thread::sleep(wait_time);
                last_ping_time = Instant::now();
            }
        });

        Ok(rx)
    }
}

// =================== Async Implementation ===================

#[cfg(feature = "async")]
pub struct WindowsAsyncPinger {
    options: PingOptions,
}

#[cfg(feature = "async")]
#[async_trait]
impl AsyncPinger for WindowsAsyncPinger {
    async fn from_options(options: PingOptions) -> Result<Self, PingCreationError> {
        Ok(Self { options })
    }

    fn parse_fn(&self) -> fn(String) -> Option<PingResult> {
        |line| {
            if line.contains("timed out") || line.contains("failure") {
                return Some(PingResult::Timeout(line));
            }
            extract_regex(&RE, line)
        }
    }

    fn ping_args(&self) -> (&str, Vec<String>) {
        unimplemented!("ping_args for WindowsAsyncPinger is not implemented")
    }

    async fn start(&self) -> Result<tokio::sync::mpsc::Receiver<PingResult>, PingCreationError> {
        let interval = self.options.interval;

        // Resolve target IP address
        let parsed_ip = resolve_target(&self.options.target)?;

        let (tx, rx) = tokio::sync::mpsc::channel(100);

        tokio::spawn(async move {
            // Create and configure async pinger
            let pinger = create_async_pinger(interval);
            let mut last_ping_time = Instant::now();

            loop {
                let buffer = Buffer::new();

                // True async ping
                let ping_future = pinger.send(parsed_ip, buffer);
                let async_result = ping_future.await;

                match async_result.result {
                    Ok(rtt) => {
                        let result = PingResult::Pong(
                            Duration::from_millis(rtt as u64),
                            format!("Reply from {}: time={}ms", parsed_ip, rtt),
                        );
                        if tx.send(result).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let should_continue = handle_ping_error(
                            e,
                            parsed_ip,
                            || {
                                // Use blocking send because we're in a closure
                                tx.blocking_send(PingResult::Timeout(format!(
                                    "Request timeout for {}",
                                    parsed_ip
                                )))
                                .is_ok()
                            },
                            || {
                                let _ = tx.blocking_send(PingResult::PingExited(
                                    std::process::ExitStatus::default(),
                                    format!("Ping error: {:?}", e),
                                ));
                                true
                            },
                        );

                        if !should_continue {
                            break;
                        }
                    }
                }

                // Calculate wait time, compensating for ping duration
                let wait_time = calculate_wait_time(last_ping_time, interval);
                tokio::time::sleep(wait_time).await;
                last_ping_time = Instant::now();
            }
        });

        Ok(rx)
    }
}
