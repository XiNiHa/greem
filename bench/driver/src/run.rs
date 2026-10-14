//! The closed loop of one cell: `concurrency` connections, each sending back
//! to back through `warmup` unrecorded requests and then a measured window.

use crate::http::{Request, connection, send};
use crate::report::Measurement;
use crate::stats::{p99, percentile};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Sample {
    elapsed: Duration,
    first_payload: Option<Duration>,
    payloads: Option<usize>,
}

/// What one connection saw inside the window.
#[derive(Default)]
struct Tally {
    samples: Vec<Sample>,
    failures: usize,
    first_error: Option<String>,
}

impl Tally {
    fn fail(&mut self, error: String) {
        self.failures += 1;
        self.first_error.get_or_insert(error);
    }

    fn absorb(&mut self, other: Tally) {
        self.samples.extend(other.samples);
        self.failures += other.failures;
        self.first_error = self.first_error.take().or(other.first_error);
    }
}

pub async fn measure(
    request: Arc<Request>,
    concurrency: usize,
    warmup: usize,
    duration: Duration,
) -> Result<Measurement, String> {
    let connections: Vec<_> = (0..concurrency)
        .map(|_| {
            let request = request.clone();
            tokio::spawn(async move {
                let connection = connection();
                let mut tally = Tally::default();
                for _ in 0..warmup {
                    if let Err(e) = send(&connection, &request).await {
                        tally.fail(e);
                    }
                }
                let deadline = Instant::now() + duration;
                while Instant::now() < deadline {
                    match send(&connection, &request).await {
                        Ok(response) if response.status == 200 => {
                            tally.samples.push(Sample {
                                elapsed: response.elapsed,
                                first_payload: response.first_payload,
                                payloads: response.payload_count(),
                            });
                        }
                        Ok(response) => tally.fail(format!("status {}", response.status)),
                        Err(e) => tally.fail(e),
                    }
                }
                tally
            })
        })
        .collect();
    let mut tally = Tally::default();
    for connection in connections {
        tally.absorb(connection.await.expect("connection task"));
    }
    if let Some(error) = tally.first_error {
        return Err(format!("{} requests failed: {error}", tally.failures));
    }
    if tally.samples.is_empty() {
        return Err("no request completed inside the window".into());
    }
    let elapsed: Vec<Duration> = tally.samples.iter().map(|s| s.elapsed).collect();
    let first: Vec<Duration> = tally
        .samples
        .iter()
        .filter_map(|s| s.first_payload)
        .collect();
    let mut payloads: Vec<usize> = tally.samples.iter().filter_map(|s| s.payloads).collect();
    payloads.sort_unstable();
    let ms = |d: Duration| d.as_secs_f64() * 1e3;
    Ok(Measurement {
        samples: tally.samples.len(),
        p50_ms: ms(percentile(&elapsed, 50.0)),
        p99_ms: p99(&elapsed).map(ms),
        rps: tally.samples.len() as f64 / duration.as_secs_f64(),
        first_payload_p50_ms: (!first.is_empty()).then(|| ms(percentile(&first, 50.0))),
        first_payload_p99_ms: p99(&first).map(ms),
        payloads: payloads.get(payloads.len() / 2).copied(),
    })
}
