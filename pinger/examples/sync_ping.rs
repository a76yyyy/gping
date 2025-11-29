//! 同步 ping 示例
//!
//! 运行方式:
//! ```bash
//! cargo run --example sync_ping
//! ```

#![allow(clippy::print_stdout, missing_docs)]

use pinger::{ping, PingOptions, PingResult};
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting sync ping to google.com...\n");

    let options = PingOptions::new("google.com", Duration::from_secs(1), None);
    let receiver = ping(options)?;

    let mut count = 0;
    let max_pings = 5;

    for result in receiver {
        match result {
            PingResult::Pong(duration, line) => {
                println!("✓ Pong #{}: {:?}", count + 1, duration);
                if !line.is_empty() {
                    println!("  Raw: {line}");
                }
                count += 1;
                if count >= max_pings {
                    println!("\nReceived {max_pings} pings, stopping...");
                    break;
                }
            }
            PingResult::Timeout(line) => {
                println!("✗ Timeout");
                if !line.is_empty() {
                    println!("  Raw: {line}");
                }
            }
            PingResult::PingExited(status, stderr) => {
                println!("\n⚠ Ping process exited: {status}");
                if !stderr.is_empty() {
                    println!("  Stderr: {stderr}");
                }
                break;
            }
            PingResult::Unknown(line) => {
                println!("? Unknown: {line}");
            }
        }
    }

    println!("\nSync ping completed!");
    Ok(())
}
