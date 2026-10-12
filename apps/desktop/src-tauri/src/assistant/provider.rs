use super::{Service, connection::Connection};
use serde_json::{Value, json};
use std::future::{Future, poll_fn};
use std::task::Poll;
use std::time::Duration;
use windfall_project::assistant::{AssistantContext, AssistantProvider, AssistantReply};

pub(super) struct Provider<'a> {
    pub config: &'a Connection,
    pub timeout: Duration,
    pub service: &'a Service,
    pub request: u64,
}
fn contains_key(value: &Value, key: &str) -> bool {
    match value {
        Value::String(text) => text.contains(key),
        Value::Array(items) => items.iter().any(|v| contains_key(v, key)),
        Value::Object(fields) => fields
            .iter()
            .any(|(name, v)| name.contains(key) || contains_key(v, key)),
        _ => false,
    }
}
impl AssistantProvider for Provider<'_> {
    fn respond(
        &self,
        prompt: &str,
        context: AssistantContext<'_>,
    ) -> Result<AssistantReply, String> {
        let url = self.config.ready()?;
        if prompt.trim().is_empty() || prompt.len() > 4096 {
            return Err("request must contain 1..4096 bytes".into());
        }
        if self.service.cancelled(self.request) {
            return Err("assistant request cancelled".into());
        }
        let schema = windfall_project::assistant_protocol::command_schema();
        let contract = json!({
            "reply": {"answer":"string, <=8192 bytes", "label":"string, <=128 bytes", "explanation":"string, <=4096 bytes", "commands":"0..128 commands matching commandSchema; [] for help/refusal"},
            "commandSchema":schema,
            "projectIds": {
                "channels":context.project.channels.iter().take(128).map(|c| c.id.0).collect::<Vec<_>>(),
                "patterns":context.project.patterns.iter().take(128).map(|p| p.id.0).collect::<Vec<_>>(),
                "mixerTracks":context.project.mixer.tracks.iter().take(128).map(|t| t.id.0).collect::<Vec<_>>()
            }
        });
        let body = serde_json::to_vec(&json!({"model":self.config.model,"stream":false,"max_tokens":4096,"messages":[
            {"role":"system","content":format!("Return only one JSON object conforming to this contract. Do not execute actions or invent IDs. Commands will be validated and reviewed by the user. Never echo credentials. {contract}")},
            {"role":"user","content":prompt}
        ]})).map_err(|_| "could not encode assistant request")?;
        if body.len() > 65_536 {
            return Err("assistant request byte limit exceeded".into());
        }
        let client = reqwest::Client::builder()
            .timeout(self.timeout)
            .connect_timeout(self.timeout.min(Duration::from_secs(5)))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|_| "could not initialize assistant HTTP client")?;
        let mut authorization =
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", self.config.key))
                .map_err(|_| "assistant key is not valid for HTTP authorization")?;
        authorization.set_sensitive(true);
        // This future is only driven on a Tauri blocking worker. Cancellation
        // wakes it and drops the HTTP future; response/errors never enter logs.
        let mut request = Box::pin(async {
            let mut response = client
                .post(url)
                .header(reqwest::header::AUTHORIZATION, authorization)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body)
                .send()
                .await
                .map_err(|e| {
                    if e.is_timeout() {
                        "assistant endpoint timed out"
                    } else {
                        "assistant endpoint request failed"
                    }
                })?;
            if !response.status().is_success() {
                return Err(
                    "assistant endpoint refused the request; check connection settings".to_owned(),
                );
            }
            const MAX_RESPONSE: usize = 600_000;
            if response
                .content_length()
                .is_some_and(|n| n > MAX_RESPONSE as u64)
            {
                return Err("assistant endpoint response byte limit exceeded".into());
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|e| {
                if e.is_timeout() {
                    "assistant endpoint timed out"
                } else {
                    "assistant response read failed"
                }
            })? {
                if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE {
                    return Err("assistant endpoint response byte limit exceeded".into());
                }
                bytes.extend_from_slice(&chunk);
            }
            let envelope: Value = serde_json::from_slice(&bytes)
                .map_err(|_| "assistant endpoint returned malformed completion JSON")?;
            let content = envelope
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str)
                .ok_or("assistant completion has no text content")?;
            let value: Value = serde_json::from_str(content)
                .map_err(|_| "assistant returned malformed reply JSON")?;
            if contains_key(&value, &self.config.key) {
                return Err("assistant returned sensitive content; reply discarded".into());
            }
            windfall_project::assistant_protocol::parse_reply(content, context.stamp)
        });
        tauri::async_runtime::block_on(poll_fn(|cx| {
            // Register before checking: cancel cannot race past the registration.
            *crate::sync::lock(&self.service.wake) = Some(cx.waker().clone());
            if self.service.cancelled(self.request) {
                return Poll::Ready(Err("assistant request cancelled".into()));
            }
            request.as_mut().poll(cx)
        }))
    }
}
