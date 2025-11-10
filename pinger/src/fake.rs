use crate::{PingCreationError, PingOptions, PingResult, Pinger};
use rand::prelude::*;
use rand::{rng, rngs::StdRng, SeedableRng};
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::Duration;

#[cfg(feature = "async")]
use crate::AsyncPinger;
#[cfg(feature = "async")]
use async_trait::async_trait;

pub struct FakePinger {
    options: PingOptions,
}

impl Pinger for FakePinger {
    fn from_options(options: PingOptions) -> Result<Self, PingCreationError>
    where
        Self: Sized,
    {
        Ok(Self { options })
    }

    fn parse_fn(&self) -> fn(String) -> Option<PingResult> {
        unimplemented!("parse for FakeParser not implemented")
    }

    fn ping_args(&self) -> (&str, Vec<String>) {
        unimplemented!("ping_args not implemented for FakePinger")
    }

    fn start(&self) -> Result<Receiver<PingResult>, PingCreationError> {
        let (tx, rx) = mpsc::channel();
        let sleep_time = self.options.interval;

        thread::spawn(move || {
            let mut random = rng();
            loop {
                let fake_seconds = random.random_range(50..150);
                let ping_result = PingResult::Pong(
                    Duration::from_millis(fake_seconds),
                    format!("Fake ping line: {fake_seconds} ms"),
                );
                if tx.send(ping_result).is_err() {
                    break;
                }

                std::thread::sleep(sleep_time);
            }
        });

        Ok(rx)
    }
}

// =================== Async Implementation ===================

#[cfg(feature = "async")]
pub struct FakeAsyncPinger {
    options: PingOptions,
}

#[cfg(feature = "async")]
#[async_trait]
impl AsyncPinger for FakeAsyncPinger {
    async fn from_options(options: PingOptions) -> Result<Self, PingCreationError>
    where
        Self: Sized,
    {
        Ok(Self { options })
    }

    fn parse_fn(&self) -> fn(String) -> Option<PingResult> {
        unimplemented!("parse for FakeAsyncPinger not implemented")
    }

    fn ping_args(&self) -> (&str, Vec<String>) {
        unimplemented!("ping_args not implemented for FakeAsyncPinger")
    }

    async fn start(&self) -> Result<tokio::sync::mpsc::Receiver<PingResult>, PingCreationError> {
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        let sleep_time = self.options.interval;

        // Use true async task
        tokio::spawn(async move {
            // Use Send-safe StdRng, seeded from thread_rng
            let mut random = StdRng::from_rng(&mut rng());

            loop {
                let fake_seconds = random.random_range(50..150);
                let ping_result = PingResult::Pong(
                    Duration::from_millis(fake_seconds),
                    format!("Fake ping line: {fake_seconds} ms"),
                );

                if tx.send(ping_result).await.is_err() {
                    break;
                }

                // Use async sleep
                tokio::time::sleep(sleep_time).await;
            }
        });

        Ok(rx)
    }
}
