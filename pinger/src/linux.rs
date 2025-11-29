//! Linux-specific ping implementation

use crate::{extract_regex, run_ping, PingCreationError, PingOptions, PingResult, Pinger};
use lazy_regex::{lazy_regex, Lazy, Regex};

#[cfg(feature = "async")]
use crate::{run_ping_async, AsyncPinger};
#[cfg(feature = "async")]
use async_trait::async_trait;

pub static UBUNTU_RE: Lazy<Regex> = lazy_regex!(r"(?i-u)time=(?P<ms>\d+)(?:\.(?P<ns>\d+))? *ms");

#[derive(Debug)]
pub enum LinuxPinger {
    // Alpine
    BusyBox(PingOptions),
    // Debian, Ubuntu, etc
    IPTools(PingOptions),
}

impl LinuxPinger {
    pub fn detect_platform_ping(options: PingOptions) -> Result<Self, PingCreationError> {
        let child = run_ping("ping", vec!["-V".to_string()])?;
        let output = child.wait_with_output()?;
        let stdout = String::from_utf8(output.stdout).map_err(|_| {
            PingCreationError::SpawnError(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid UTF-8 in ping stdout",
            ))
        })?;
        let stderr = String::from_utf8(output.stderr).map_err(|_| {
            PingCreationError::SpawnError(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid UTF-8 in ping stderr",
            ))
        })?;

        if stderr.contains("BusyBox") {
            Ok(LinuxPinger::BusyBox(options))
        } else if stdout.contains("iputils") {
            Ok(LinuxPinger::IPTools(options))
        } else if stdout.contains("inetutils") {
            Err(PingCreationError::NotSupported {
                alternative: "Please use iputils ping, not inetutils.".to_string(),
            })
        } else {
            let first_two_lines_stderr: Vec<String> =
                stderr.lines().take(2).map(str::to_string).collect();
            let first_two_lines_stout: Vec<String> =
                stdout.lines().take(2).map(str::to_string).collect();
            Err(PingCreationError::UnknownPing {
                stdout: first_two_lines_stout,
                stderr: first_two_lines_stderr,
            })
        }
    }
}

impl Pinger for LinuxPinger {
    fn from_options(options: PingOptions) -> Result<Self, PingCreationError>
    where
        Self: Sized,
    {
        Self::detect_platform_ping(options)
    }

    #[allow(clippy::print_stderr)]
    fn parse_fn(&self) -> fn(String) -> Option<PingResult> {
        |line| {
            #[cfg(test)]
            eprintln!("Got line {line}");
            if line.starts_with("64 bytes from") {
                return extract_regex(&UBUNTU_RE, line);
            } else if line.starts_with("no answer yet") {
                return Some(PingResult::Timeout(line));
            }
            None
        }
    }

    fn ping_args(&self) -> (&str, Vec<String>) {
        match self {
            // Alpine doesn't support timeout notifications, so we don't add the -O flag here.
            LinuxPinger::BusyBox(options) => {
                let cmd = if options.target.is_ipv6() {
                    "ping6"
                } else {
                    "ping"
                };

                let mut args = vec![
                    options.target.to_string(),
                    format!("-i{:.1}", options.interval.as_millis() as f32 / 1_000_f32),
                ];

                if let Some(raw_args) = &options.raw_arguments {
                    args.extend(raw_args.iter().cloned());
                }

                (cmd, args)
            }
            LinuxPinger::IPTools(options) => {
                let cmd = if options.target.is_ipv6() {
                    "ping6"
                } else {
                    "ping"
                };

                // The -O flag ensures we "no answer yet" messages from ping
                // See https://superuser.com/questions/270083/linux-ping-show-time-out
                let mut args = vec![
                    "-O".to_string(),
                    format!("-i{:.1}", options.interval.as_millis() as f32 / 1_000_f32),
                ];
                if let Some(interface) = &options.interface {
                    args.push("-I".into());
                    args.push(interface.clone());
                }
                if let Some(raw_args) = &options.raw_arguments {
                    args.extend(raw_args.iter().cloned());
                }

                args.push(options.target.to_string());
                (cmd, args)
            }
        }
    }
}

// =================== Async Implementation ===================

#[cfg(feature = "async")]
#[derive(Debug)]
pub enum LinuxAsyncPinger {
    // Alpine
    BusyBox(PingOptions),
    // Debian, Ubuntu, etc
    IPTools(PingOptions),
}

#[cfg(feature = "async")]
impl LinuxAsyncPinger {
    pub async fn detect_platform_ping(options: PingOptions) -> Result<Self, PingCreationError> {
        let child = run_ping_async("ping", vec!["-V".to_string()]).await?;
        let output = child.wait_with_output().await?;
        let stdout = String::from_utf8(output.stdout).map_err(|_| {
            PingCreationError::SpawnError(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid UTF-8 in ping stdout",
            ))
        })?;
        let stderr = String::from_utf8(output.stderr).map_err(|_| {
            PingCreationError::SpawnError(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid UTF-8 in ping stderr",
            ))
        })?;

        if stderr.contains("BusyBox") {
            Ok(LinuxAsyncPinger::BusyBox(options))
        } else if stdout.contains("iputils") {
            Ok(LinuxAsyncPinger::IPTools(options))
        } else if stdout.contains("inetutils") {
            Err(PingCreationError::NotSupported {
                alternative: "Please use iputils ping, not inetutils.".to_string(),
            })
        } else {
            let first_two_lines_stderr: Vec<String> =
                stderr.lines().take(2).map(str::to_string).collect();
            let first_two_lines_stout: Vec<String> =
                stdout.lines().take(2).map(str::to_string).collect();
            Err(PingCreationError::UnknownPing {
                stdout: first_two_lines_stout,
                stderr: first_two_lines_stderr,
            })
        }
    }
}

#[cfg(feature = "async")]
#[async_trait]
impl AsyncPinger for LinuxAsyncPinger {
    async fn from_options(options: PingOptions) -> Result<Self, PingCreationError>
    where
        Self: Sized,
    {
        Self::detect_platform_ping(options).await
    }

    #[allow(clippy::print_stderr)]
    fn parse_fn(&self) -> fn(String) -> Option<PingResult> {
        |line| {
            #[cfg(test)]
            eprintln!("Got line {line}");
            if line.starts_with("64 bytes from") {
                return extract_regex(&UBUNTU_RE, line);
            } else if line.starts_with("no answer yet") {
                return Some(PingResult::Timeout(line));
            }
            None
        }
    }

    fn ping_args(&self) -> (&str, Vec<String>) {
        match self {
            // Alpine doesn't support timeout notifications, so we don't add the -O flag here.
            LinuxAsyncPinger::BusyBox(options) => {
                let cmd = if options.target.is_ipv6() {
                    "ping6"
                } else {
                    "ping"
                };

                let mut args = vec![
                    options.target.to_string(),
                    format!("-i{:.1}", options.interval.as_millis() as f32 / 1_000_f32),
                ];

                if let Some(raw_args) = &options.raw_arguments {
                    args.extend(raw_args.iter().cloned());
                }

                (cmd, args)
            }
            LinuxAsyncPinger::IPTools(options) => {
                let cmd = if options.target.is_ipv6() {
                    "ping6"
                } else {
                    "ping"
                };

                // The -O flag ensures we "no answer yet" messages from ping
                // See https://superuser.com/questions/270083/linux-ping-show-time-out
                let mut args = vec![
                    "-O".to_string(),
                    format!("-i{:.1}", options.interval.as_millis() as f32 / 1_000_f32),
                ];
                if let Some(interface) = &options.interface {
                    args.push("-I".into());
                    args.push(interface.clone());
                }
                if let Some(raw_args) = &options.raw_arguments {
                    args.extend(raw_args.iter().cloned());
                }

                args.push(options.target.to_string());
                (cmd, args)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(target_os = "linux")]
    fn test_linux_detection() {
        use super::*;
        use os_info::Type;
        use std::time::Duration;

        let platform = LinuxPinger::detect_platform_ping(PingOptions::new(
            "foo.com".to_string(),
            Duration::from_secs(1),
            None,
        ))
        .unwrap();
        match os_info::get().os_type() {
            Type::Alpine => {
                assert!(matches!(platform, LinuxPinger::BusyBox(_)))
            }
            Type::Ubuntu => {
                assert!(matches!(platform, LinuxPinger::IPTools(_)))
            }
            _ => {}
        }
    }
}
