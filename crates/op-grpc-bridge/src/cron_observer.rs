//! Event-driven observation of autonomous native cron executions.
//!
//! `/api/events` is a wakeup source only. The event payload is never persisted,
//! logged, or projected. Every accepted `cron_result` causes one complete,
//! bounded `/api/cron` readback, which MutationEngine commits before notifying
//! its consumers.

use crate::{
    cron_dispatch, cron_gateway::TchedRouterCronGateway, cron_methods::CronMethods,
    mutation_engine::MutationEngine, tched_router_runtime::TchedRouterRuntimeClient,
};
use anyhow::{Context, Result, bail, ensure};
use futures::StreamExt;
use std::{future::Future, sync::Arc, time::Duration};

const MAX_SSE_FRAME_BYTES: usize = 256 * 1024;
const MAX_WAKEUPS_PER_CHUNK: usize = 1024;
const MAX_RECONNECTS: usize = 8;
const RECONNECT_DELAYS: [Duration; MAX_RECONNECTS] = [
    Duration::from_millis(250),
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(5),
    Duration::from_secs(5),
    Duration::from_secs(5),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wakeup {
    CronResult,
    Gap,
}

#[derive(Default)]
struct SseDecoder {
    pending: Vec<u8>,
}

impl SseDecoder {
    fn push(&mut self, chunk: &[u8]) -> Result<Vec<Wakeup>> {
        let mut wakeups = Vec::new();
        for byte in chunk {
            self.pending.push(*byte);
            let delimiter_len = if self.pending.ends_with(b"\r\n\r\n") {
                Some(4)
            } else if self.pending.ends_with(b"\n\n") {
                Some(2)
            } else {
                None
            };
            ensure!(
                self.pending.len() <= MAX_SSE_FRAME_BYTES,
                "native event frame exceeds {MAX_SSE_FRAME_BYTES} bytes"
            );
            let Some(delimiter_len) = delimiter_len else {
                continue;
            };
            self.pending.truncate(self.pending.len() - delimiter_len);
            let frame = std::mem::take(&mut self.pending);
            if let Some(wakeup) = parse_frame(&frame)? {
                wakeups.push(wakeup);
                ensure!(
                    wakeups.len() <= MAX_WAKEUPS_PER_CHUNK,
                    "native event chunk contains more than {MAX_WAKEUPS_PER_CHUNK} state wakeups"
                );
            }
        }
        Ok(wakeups)
    }

    fn finish(self) -> Result<()> {
        ensure!(
            self.pending.iter().all(u8::is_ascii_whitespace),
            "native event stream ended with a truncated frame"
        );
        Ok(())
    }
}

fn parse_frame(frame: &[u8]) -> Result<Option<Wakeup>> {
    let frame = std::str::from_utf8(frame).context("native event frame is not UTF-8")?;
    let mut event_name = None;
    let mut data = String::new();
    for line in frame.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.starts_with(':') || line.is_empty() {
            continue;
        }
        if let Some(value) = line.strip_prefix("event:") {
            event_name = Some(value.trim());
        } else if let Some(value) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(value.strip_prefix(' ').unwrap_or(value));
            ensure!(
                data.len() <= MAX_SSE_FRAME_BYTES,
                "native event data exceeds {MAX_SSE_FRAME_BYTES} bytes"
            );
        }
    }
    if event_name == Some("gap") {
        return Ok(Some(Wakeup::Gap));
    }
    if data.is_empty() {
        return Ok(None);
    }

    // Parse only long enough to identify the wakeup. In particular, never
    // retain, return, or log the native `output` field carried by cron_result.
    let value: serde_json::Value =
        serde_json::from_str(&data).context("native event data is invalid JSON")?;
    if value.get("type").and_then(serde_json::Value::as_str) != Some("cron_result") {
        return Ok(None);
    }
    let job_id = value
        .get("job_id")
        .and_then(serde_json::Value::as_str)
        .context("native cron_result has no job_id")?;
    ensure!(
        !job_id.is_empty() && job_id.len() <= 256 && !job_id.chars().any(char::is_control),
        "native cron_result has an invalid job_id"
    );
    Ok(Some(Wakeup::CronResult))
}

async fn observe_connection(
    engine: &MutationEngine,
    client: &TchedRouterRuntimeClient,
    methods: &CronMethods,
) -> Result<()> {
    // Subscribe before the first hydration. Events produced during the bounded
    // readback queue in the native response rather than falling into a gap.
    let response = client
        .open_gateway_event_stream()
        .await
        .map_err(|_| anyhow::anyhow!("native cron event subscription is unavailable"))?;
    drive_event_stream(response.bytes_stream(), || async {
        cron_dispatch::publish_observed_snapshot(engine, methods).await
    })
    .await
}

async fn drive_event_stream<S, B, E, P, F>(stream: S, mut publish: P) -> Result<()>
where
    S: futures::Stream<Item = std::result::Result<B, E>>,
    B: AsRef<[u8]>,
    E: std::fmt::Display,
    P: FnMut() -> F,
    F: Future<Output = Result<()>>,
{
    // The caller has already established the subscription. This initial
    // complete readback closes the subscribe/hydrate race.
    publish()
        .await
        .context("initial native cron snapshot publication failed")?;
    let mut decoder = SseDecoder::default();
    futures::pin_mut!(stream);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| anyhow::anyhow!("native cron event stream failed"))?;
        for wakeup in decoder.push(chunk.as_ref())? {
            match wakeup {
                Wakeup::CronResult => {
                    publish()
                        .await
                        .context("cron_result snapshot publication failed")?;
                }
                Wakeup::Gap => bail!("native cron event stream reported a delivery gap"),
            }
        }
    }
    decoder.finish()?;
    bail!("native cron event stream closed")
}

/// Observe native cron wakeups with a finite reconnect budget. Reconnection is
/// attempted only after an explicit stream close/error/gap. Every newly opened
/// stream is established before its rehydration read; there is no polling or
/// heartbeat-triggered state read.
pub(crate) async fn observe(engine: Arc<MutationEngine>) -> Result<()> {
    let client = TchedRouterRuntimeClient::from_env();
    let gateway = Arc::new(TchedRouterCronGateway::from_client(client.clone()));
    let methods = CronMethods::new(gateway);

    let mut last_error = None;
    for attempt in 0..=MAX_RECONNECTS {
        if attempt > 0 {
            tokio::time::sleep(RECONNECT_DELAYS[attempt - 1]).await;
        }
        match observe_connection(&engine, &client, &methods).await {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("native cron observer ended")))
        .context("native cron observer exhausted its bounded reconnect budget")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragmented_cron_result_is_one_wakeup_and_output_is_not_returned() {
        let mut decoder = SseDecoder::default();
        assert!(
            decoder
                .push(b"data: {\"type\":\"cron_result\",\"job_id\":\"job-1\",\"out")
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            decoder.push(b"put\":\"private result\"}\n\n").unwrap(),
            vec![Wakeup::CronResult]
        );
        decoder.finish().unwrap();
    }

    #[test]
    fn explicit_gap_is_not_treated_as_a_state_event() {
        let mut decoder = SseDecoder::default();
        assert_eq!(
            decoder.push(b"event: gap\ndata: 7\n\n").unwrap(),
            vec![Wakeup::Gap]
        );
    }

    #[test]
    fn unrelated_events_and_keepalives_do_not_wake_state_reads() {
        let mut decoder = SseDecoder::default();
        assert!(decoder.push(b": keepalive\n\n").unwrap().is_empty());
        assert!(
            decoder
                .push(b"data: {\"type\":\"tool_call\",\"output\":\"private\"}\n\n")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn malformed_oversized_and_truncated_frames_fail_closed() {
        let mut malformed = SseDecoder::default();
        assert!(malformed.push(b"data: not-json\n\n").is_err());

        let mut oversized = SseDecoder::default();
        assert!(
            oversized
                .push(&vec![b'x'; MAX_SSE_FRAME_BYTES + 1])
                .is_err()
        );

        let mut truncated = SseDecoder::default();
        truncated.push(b"data: {\"type\":").unwrap();
        assert!(truncated.finish().is_err());
    }

    #[test]
    fn multiple_cron_results_preserve_one_wakeup_per_real_frame() {
        let mut decoder = SseDecoder::default();
        let frames = b"data: {\"type\":\"cron_result\",\"job_id\":\"a\"}\n\ndata: {\"type\":\"cron_result\",\"job_id\":\"b\"}\n\n";
        assert_eq!(
            decoder.push(frames).unwrap(),
            vec![Wakeup::CronResult, Wakeup::CronResult]
        );
    }

    #[tokio::test]
    async fn driver_hydrates_once_then_reads_once_per_cron_result() {
        let stream = futures::stream::iter(vec![Ok::<_, &'static str>(
            b"data: {\"type\":\"tool_call\"}\n\ndata: {\"type\":\"cron_result\",\"job_id\":\"a\",\"output\":\"secret\"}\n\ndata: {\"type\":\"cron_result\",\"job_id\":\"b\"}\n\n"
                .as_slice(),
        )]);
        let publications = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let publish = publications.clone();
        let result = drive_event_stream(stream, move || {
            let publish = publish.clone();
            async move {
                publish.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }
        })
        .await;
        assert!(
            result.is_err(),
            "finite stream closure requires rehydration"
        );
        assert_eq!(
            publications.load(std::sync::atomic::Ordering::SeqCst),
            3,
            "one initial hydration plus one full read per cron_result"
        );
    }

    #[tokio::test]
    async fn explicit_gap_stops_before_following_frame() {
        let stream = futures::stream::iter(vec![Ok::<_, &'static str>(
            b"event: gap\ndata: 1\n\ndata: {\"type\":\"cron_result\",\"job_id\":\"late\"}\n\n"
                .as_slice(),
        )]);
        let publications = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let publish = publications.clone();
        assert!(
            drive_event_stream(stream, move || {
                let publish = publish.clone();
                async move {
                    publish.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(())
                }
            })
            .await
            .is_err()
        );
        assert_eq!(
            publications.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "gap forces reconnect/rehydration without consuming later frames"
        );
    }
}
