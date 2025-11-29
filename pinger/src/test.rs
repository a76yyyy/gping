#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::indexing_slicing
)]
mod tests {
    #[cfg(unix)]
    use crate::bsd::BSDPinger;
    #[cfg(unix)]
    use crate::linux::LinuxPinger;
    #[cfg(unix)]
    use crate::macos::MacOSPinger;
    #[cfg(windows)]
    use crate::windows::WindowsPinger;
    use crate::{PingOptions, PingResult, Pinger};
    use anyhow::bail;
    use ntest::timeout;
    use std::time::Duration;

    const IS_GHA: bool = option_env!("GITHUB_ACTIONS").is_some();

    #[test]
    #[timeout(20_000)]
    fn test_integration_any() {
        run_integration_test(&PingOptions::new(
            "tomforb.es",
            Duration::from_millis(500),
            None,
        ))
        .unwrap();
    }
    #[test]
    #[timeout(20_000)]
    fn test_integration_ipv4() {
        run_integration_test(&PingOptions::new_ipv4(
            "tomforb.es",
            Duration::from_millis(500),
            None,
        ))
        .unwrap();
    }
    #[test]
    #[timeout(20_000)]
    fn test_integration_ip6() {
        let res = run_integration_test(&PingOptions::new_ipv6(
            "::1",
            Duration::from_millis(500),
            None,
        ));

        // IPv6 tests are allowed to fail when IPv6 is not available
        // Check if the error is specifically about IPv6 not being available
        match res {
            Ok(()) => {
                // Test passed, IPv6 is available
            }
            Err(e) => {
                let error_msg = e.to_string();
                // Allow failure if it's a network-related IPv6 error
                if error_msg.contains("No route to host")
                    || error_msg.contains("Network is unreachable")
                    || error_msg.contains("Address family not supported")
                {
                    eprintln!("IPv6 test skipped: IPv6 is not available on this system");
                } else if IS_GHA {
                    // On CI, allow any IPv6 failure
                    eprintln!("IPv6 test failed on CI (expected): {e:?}");
                } else {
                    // On local machines with unexpected errors, fail the test
                    panic!("Unexpected IPv6 test failure: {e:?}");
                }
            }
        }
    }

    fn run_integration_test(options: &PingOptions) -> anyhow::Result<()> {
        let stream = crate::ping(options.clone())?;

        let mut success = 0;
        let mut errors = 0;

        for message in stream.into_iter().take(3) {
            match message {
                PingResult::Pong(_, m) | PingResult::Timeout(m) => {
                    eprintln!("Message: {m}");
                    success += 1;
                }
                PingResult::Unknown(line) => {
                    eprintln!("Unknown line: {line}");
                    errors += 1;
                }
                PingResult::PingExited(code, stderr) => {
                    bail!("Ping exited with code: {}, stderr: {}", code, stderr);
                }
            }
        }
        assert_eq!(success, 3, "Success != 3 with opts {options:?}");
        assert_eq!(errors, 0, "Errors != 0 with opts {options:?}");
        Ok(())
    }

    fn opts() -> PingOptions {
        PingOptions::new("foo".to_string(), Duration::from_secs(1), None)
    }

    fn test_parser<T: Pinger>(contents: &str) {
        let pinger = T::from_options(opts()).unwrap();
        run_parser_test(contents, &pinger);
    }

    fn run_parser_test(contents: &str, pinger: &impl Pinger) {
        let parser = pinger.parse_fn();
        let test_file: Vec<&str> = contents.split("-----").collect();
        let input = test_file[0].lines();
        let expected: Vec<&str> = test_file[1].lines().collect();
        let parsed: Vec<Option<PingResult>> = input.map(|l| parser(l.to_string())).collect();

        assert_eq!(
            parsed.len(),
            expected.len(),
            "Parsed: {:?}, Expected: {:?}",
            &parsed,
            &expected
        );

        for (idx, (output, expected)) in parsed.into_iter().zip(expected).enumerate() {
            if let Some(value) = output {
                assert_eq!(
                    format!("{value}").trim(),
                    expected.trim(),
                    "Failed at idx {idx}"
                );
            } else {
                assert_eq!("None", expected.trim(), "Failed at idx {idx}");
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn macos() {
        test_parser::<MacOSPinger>(include_str!("tests/macos.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn freebsd() {
        test_parser::<BSDPinger>(include_str!("tests/bsd.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn dragonfly() {
        test_parser::<BSDPinger>(include_str!("tests/bsd.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn openbsd() {
        test_parser::<BSDPinger>(include_str!("tests/bsd.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn netbsd() {
        test_parser::<BSDPinger>(include_str!("tests/bsd.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn ubuntu() {
        run_parser_test(
            include_str!("tests/ubuntu.txt"),
            &LinuxPinger::IPTools(opts()),
        );
    }

    #[cfg(unix)]
    #[test]
    fn debian() {
        run_parser_test(
            include_str!("tests/debian.txt"),
            &LinuxPinger::IPTools(opts()),
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows() {
        test_parser::<WindowsPinger>(include_str!("tests/windows.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn android() {
        run_parser_test(
            include_str!("tests/android.txt"),
            &LinuxPinger::BusyBox(opts()),
        );
    }

    #[cfg(unix)]
    #[test]
    fn alpine() {
        run_parser_test(
            include_str!("tests/alpine.txt"),
            &LinuxPinger::BusyBox(opts()),
        );
    }
}

#[cfg(all(test, feature = "async"))]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr
)]
mod async_tests {
    use crate::{ping_async, PingOptions, PingResult};
    use anyhow::bail;
    use ntest::timeout;
    use std::time::Duration;
    const IS_GHA: bool = option_env!("GITHUB_ACTIONS").is_some();

    #[tokio::test]
    #[timeout(20_000)]
    async fn test_async_integration_any() {
        run_async_integration_test(PingOptions::new(
            "tomforb.es",
            Duration::from_millis(500),
            None,
        ))
        .await
        .unwrap();
    }

    #[tokio::test]
    #[timeout(20_000)]
    async fn test_async_integration_ipv4() {
        run_async_integration_test(PingOptions::new_ipv4(
            "tomforb.es",
            Duration::from_millis(500),
            None,
        ))
        .await
        .unwrap();
    }

    #[tokio::test]
    #[timeout(20_000)]
    async fn test_async_integration_ipv6() {
        let res = run_async_integration_test(PingOptions::new_ipv6(
            "::1",
            Duration::from_millis(500),
            None,
        ))
        .await;

        // IPv6 tests are allowed to fail when IPv6 is not available
        // Check if the error is specifically about IPv6 not being available
        match res {
            Ok(()) => {
                // Test passed, IPv6 is available
            }
            Err(e) => {
                let error_msg = e.to_string();
                // Allow failure if it's a network-related IPv6 error
                if error_msg.contains("No route to host")
                    || error_msg.contains("Network is unreachable")
                    || error_msg.contains("Address family not supported")
                {
                    eprintln!("IPv6 test skipped: IPv6 is not available on this system");
                } else if IS_GHA {
                    // On CI, allow any IPv6 failure
                    eprintln!("IPv6 test failed on CI (expected): {e:?}");
                } else {
                    // On local machines with unexpected errors, fail the test
                    panic!("Unexpected IPv6 test failure: {e:?}");
                }
            }
        }
    }

    async fn run_async_integration_test(options: PingOptions) -> anyhow::Result<()> {
        let mut stream = ping_async(options.clone()).await?;

        let mut success = 0;
        let mut errors = 0;

        for _ in 0..3 {
            match stream.recv().await {
                Some(PingResult::Pong(_, m) | PingResult::Timeout(m)) => {
                    eprintln!("Message: {m}");
                    success += 1;
                }
                Some(PingResult::Unknown(line)) => {
                    eprintln!("Unknown line: {line}");
                    errors += 1;
                }
                Some(PingResult::PingExited(code, stderr)) => {
                    bail!("Ping exited with code: {}, stderr: {}", code, stderr);
                }
                None => {
                    bail!("Stream ended prematurely");
                }
            }
        }

        assert_eq!(success, 3, "Success != 3 with opts {options:?}");
        assert_eq!(errors, 0, "Errors != 0 with opts {options:?}");
        Ok(())
    }

    #[cfg(feature = "fake-ping")]
    #[tokio::test]
    #[timeout(10_000)]
    async fn test_async_fake_ping() {
        std::env::set_var("PINGER_FAKE_PING", "1");

        let options = PingOptions::new("fake.example.com", Duration::from_millis(100), None);
        let mut stream = ping_async(options)
            .await
            .expect("Failed to start fake ping");

        let mut count = 0;
        for _ in 0..5 {
            match stream.recv().await {
                Some(PingResult::Pong(duration, _)) => {
                    eprintln!("Fake ping: {duration:?}");
                    count += 1;
                }
                Some(other) => {
                    panic!("Unexpected result: {other:?}");
                }
                None => break,
            }
        }

        std::env::remove_var("PINGER_FAKE_PING");
        assert_eq!(count, 5, "Should receive 5 fake pings");
    }
}
