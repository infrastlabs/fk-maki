use maki_providers::provider::Provider;
use maki_providers::retry::{MAX_TIMEOUT_RETRIES, RetryState};
use maki_providers::{Message, Model, ProviderEvent, RequestOptions, StreamResponse};
use maki_storage::id::SessionRef;
use serde_json::Value;
use tracing::{debug, warn};

use crate::cancel::CancelToken;
use crate::{AgentError, AgentEvent, EventSender};

async fn forward_provider_events(prx: flume::Receiver<ProviderEvent>, event_tx: &EventSender) {
    while let Ok(pe) = prx.recv_async().await {
        let ae = match pe {
            ProviderEvent::TextDelta { text } => AgentEvent::TextDelta { text },
            ProviderEvent::ThinkingDelta { text } => AgentEvent::ThinkingDelta { text },
            ProviderEvent::ToolUseStart { id, name } => AgentEvent::ToolPending { id, name },
            ProviderEvent::PromptProgress {
                processed,
                total,
                cache,
            } => AgentEvent::PromptProgress {
                processed,
                total,
                cache,
            },
        };
        if event_tx.send(ae).is_err() {
            break;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn stream_with_retry(
    provider: &dyn Provider,
    model: &Model,
    messages: &[Message],
    system: &str,
    tools: &Value,
    event_tx: &EventSender,
    cancel: &CancelToken,
    opts: RequestOptions,
    session_id: Option<&SessionRef>,
) -> Result<StreamResponse, AgentError> {
    let opts = opts.clamped(model);
    let messages = maki_providers::adapt_images_for_model(model, messages);
    let messages = &*messages;
    let mut retry = RetryState::new();
    let mut attempt_num: u32 = 0;
    loop {
        attempt_num += 1;
        debug!(attempt = attempt_num, model = %model.id, "[RETRY] attempt {attempt_num}");
        let (ptx, prx) = flume::unbounded();
        let forwarder = smol::spawn({
            let event_tx = event_tx.clone();
            async move { forward_provider_events(prx, &event_tx).await }
        });
        let result = futures_lite::future::race(
            provider.stream_message(model, messages, system, tools, &ptx, opts, session_id),
            async {
                cancel.cancelled().await;
                Err(AgentError::Cancelled)
            },
        )
        .await;
        drop(ptx);
        let _ = forwarder.await;
        match result {
            Ok(r) => {
                debug!(attempt = attempt_num, "[RETRY] success on attempt {attempt_num}");
                return Ok(r);
            }
            Err(AgentError::Cancelled) => return Err(AgentError::Cancelled),
            Err(e) if e.is_retryable() => {
                if e.should_rotate_key()
                    && let Ok(true) = provider.rotate_key().await
                {
                    warn!("rotated API key after error: {e}");
                }
                let (attempt, delay) = retry.next_delay();
                if matches!(e, AgentError::Timeout { .. }) && attempt > MAX_TIMEOUT_RETRIES {
                    warn!(attempt, error = %e, "[RETRY] max retries exhausted");
                    return Err(e);
                }
                let delay_ms = delay.as_millis() as u64;
                warn!(attempt, delay_ms, error = %e, "[RETRY] retrying after error");
                event_tx.send(AgentEvent::Retry {
                    attempt,
                    message: e.retry_message(),
                    delay_ms,
                })?;
                futures_lite::future::race(
                    async {
                        smol::Timer::after(delay).await;
                    },
                    cancel.cancelled(),
                )
                .await;
                if cancel.is_cancelled() {
                    return Err(AgentError::Cancelled);
                }
            }
            Err(e) => {
                debug!(error = %e, "[RETRY] non-retryable error");
                return Err(e);
            }
        }
    }
}
