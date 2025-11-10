/// 异步 ping 示例
///
/// 运行方式:
/// ```bash
/// cargo run --example async_ping --features async
/// ```
use pinger::{ping_async, PingOptions, PingResult};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting async ping to google.com...\n");

    let options = PingOptions::new("google.com", Duration::from_secs(1), None);
    let mut receiver = ping_async(options).await?;

    let mut count = 0;
    let max_pings = 5;

    while let Some(result) = receiver.recv().await {
        match result {
            PingResult::Pong(duration, line) => {
                println!("✓ Pong #{}: {:?}", count + 1, duration);
                if !line.is_empty() {
                    println!("  Raw: {}", line);
                }
                count += 1;
                if count >= max_pings {
                    println!("\nReceived {} pings, stopping...", max_pings);
                    break;
                }
            }
            PingResult::Timeout(line) => {
                println!("✗ Timeout");
                if !line.is_empty() {
                    println!("  Raw: {}", line);
                }
            }
            PingResult::PingExited(status, stderr) => {
                println!("\n⚠ Ping process exited: {}", status);
                if !stderr.is_empty() {
                    println!("  Stderr: {}", stderr);
                }
                break;
            }
            PingResult::Unknown(line) => {
                println!("? Unknown: {}", line);
            }
        }
    }

    println!("\nAsync ping completed!");
    Ok(())
}
