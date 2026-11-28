//! Bounded, read-only HTTP transport for plugins whose method contracts are
//! generated from a pinned upstream OpenAPI document (`prometheus`, `grafana`).
//!
//! The generated `<plugin>/api_types.rs` modules own every request/response shape and
//! the operation table (`OPERATIONS`). This module only moves bytes: it fills
//! path templates from typed arguments, encodes the remaining typed arguments
//! as a query string, enforces the time and size bounds, and decodes the body
//! into the generated output type. The upstream base URL comes from the
//! service's deploy-time environment, never from method input.

use std::time::Duration;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

/// Typed failure of a generated-contract upstream call.
#[derive(Debug, thiserror::Error)]
pub enum UpstreamCallError {
    #[error("unknown {service} method '{method}'; no canonical backend dispatch")]
    UnknownMethod {
        service: &'static str,
        method: String,
    },
    #[error("invalid {service}.{method} arguments: {message}")]
    InvalidInput {
        service: &'static str,
        method: String,
        message: String,
    },
    #[error("{service} endpoint is invalid: {message}")]
    InvalidEndpoint {
        service: &'static str,
        message: String,
    },
    #[error("{service} request timed out after {timeout_ms} ms")]
    Timeout {
        service: &'static str,
        timeout_ms: u128,
    },
    #[error("{service} response exceeds {limit_bytes} bytes")]
    ResponseTooLarge {
        service: &'static str,
        limit_bytes: usize,
    },
    #[error("{service} result exceeds {limit} {unit} (observed {observed})")]
    ResultLimit {
        service: &'static str,
        unit: &'static str,
        limit: usize,
        observed: usize,
    },
    #[error("{service} returned HTTP {status} ({error_type}): {message}")]
    Api {
        service: &'static str,
        status: u16,
        error_type: String,
        message: String,
    },
    #[error("{service} returned HTTP {status}: {body}")]
    Upstream {
        service: &'static str,
        status: u16,
        body: Value,
    },
    #[error("{service} transport error: {message}")]
    Transport {
        service: &'static str,
        message: String,
    },
    #[error("{service} response does not match the generated {output} contract: {message}")]
    Contract {
        service: &'static str,
        output: &'static str,
        message: String,
    },
}

/// One configured upstream API root with its bounds.
pub(crate) struct UpstreamEndpoint {
    service: &'static str,
    client: reqwest::Client,
    base: reqwest::Url,
    bearer: Option<String>,
    timeout: Duration,
    max_response_bytes: usize,
}

impl UpstreamEndpoint {
    /// `raw_base` is the server root (e.g. `http://127.0.0.1:9090`);
    /// `api_prefix` is the spec's server path (e.g. `/api/v1`).
    pub(crate) fn new(
        service: &'static str,
        raw_base: &str,
        api_prefix: &str,
        bearer: Option<String>,
        timeout: Duration,
        max_response_bytes: usize,
    ) -> Result<Self, UpstreamCallError> {
        let invalid = |message: &str| UpstreamCallError::InvalidEndpoint {
            service,
            message: message.to_string(),
        };
        let mut base = reqwest::Url::parse(raw_base.trim_end_matches('/'))
            .map_err(|error| invalid(&error.to_string()))?;
        if !matches!(base.scheme(), "http" | "https") {
            return Err(invalid("must use http or https"));
        }
        if !base.username().is_empty() || base.password().is_some() {
            return Err(invalid("must not contain userinfo"));
        }
        if base.query().is_some() || base.fragment().is_some() {
            return Err(invalid("must not contain query or fragment"));
        }
        if base.host_str().is_none() {
            return Err(invalid("must name a host"));
        }
        {
            let mut segments = base
                .path_segments_mut()
                .map_err(|()| invalid("cannot be a base URL"))?;
            segments.pop_if_empty();
            for segment in api_prefix.split('/').filter(|s| !s.is_empty()) {
                segments.push(segment);
            }
        }
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .connect_timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| UpstreamCallError::Transport {
                service,
                message: error.to_string(),
            })?;
        Ok(Self {
            service,
            client,
            base,
            bearer: bearer.filter(|token| !token.is_empty()),
            timeout,
            max_response_bytes,
        })
    }

    pub(crate) fn service(&self) -> &'static str {
        self.service
    }

    pub(crate) fn base_url(&self) -> &str {
        self.base.as_str()
    }

    /// Build the request URL for `path_template` (e.g. `/label/{name}/values`):
    /// template parameters are taken from `args` and percent-encoded as single
    /// path segments; the remaining arguments become query pairs (arrays repeat
    /// the key, `null` is omitted).
    pub(crate) fn url_for(
        &self,
        method: &str,
        path_template: &str,
        args: &Value,
    ) -> Result<reqwest::Url, UpstreamCallError> {
        let invalid = |message: String| UpstreamCallError::InvalidInput {
            service: self.service,
            method: method.to_string(),
            message,
        };
        let mut remaining = match args {
            Value::Object(map) => map.clone(),
            Value::Null => serde_json::Map::new(),
            _ => return Err(invalid("arguments must be an object".into())),
        };
        let mut url = self.base.clone();
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|()| invalid("endpoint cannot carry a path".into()))?;
            for segment in path_template.split('/').filter(|s| !s.is_empty()) {
                if let Some(name) = segment.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                    let value = remaining
                        .remove(name)
                        .ok_or_else(|| invalid(format!("missing path parameter '{name}'")))?;
                    let value = match value {
                        Value::String(s) => s,
                        Value::Number(n) => n.to_string(),
                        _ => {
                            return Err(invalid(format!(
                                "path parameter '{name}' must be a scalar"
                            )));
                        }
                    };
                    if value.is_empty() || value == "." || value == ".." || value.contains('/') {
                        return Err(invalid(format!(
                            "path parameter '{name}' must be one concrete segment"
                        )));
                    }
                    segments.push(&value);
                } else {
                    segments.push(segment);
                }
            }
        }
        {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in &remaining {
                match value {
                    Value::Null => {}
                    Value::Array(items) => {
                        for item in items {
                            pairs.append_pair(
                                key,
                                &scalar(item).ok_or_else(|| {
                                    invalid(format!(
                                        "query parameter '{key}' items must be scalars"
                                    ))
                                })?,
                            );
                        }
                    }
                    other => {
                        pairs.append_pair(
                            key,
                            &scalar(other).ok_or_else(|| {
                                invalid(format!(
                                    "query parameter '{key}' must be a scalar or array"
                                ))
                            })?,
                        );
                    }
                }
            }
        }
        if url.query() == Some("") {
            url.set_query(None);
        }
        Ok(url)
    }

    /// GET `url` and return the JSON body, bounded by the endpoint's timeout
    /// and response size. Non-2xx responses become `Upstream` errors carrying
    /// the (bounded) JSON body for the caller to type.
    pub(crate) async fn get_json(&self, url: reqwest::Url) -> Result<Value, UpstreamCallError> {
        let service = self.service;
        let transport = |error: reqwest::Error| {
            if error.is_timeout() {
                UpstreamCallError::Timeout {
                    service,
                    timeout_ms: self.timeout.as_millis(),
                }
            } else {
                UpstreamCallError::Transport {
                    service,
                    message: error.to_string(),
                }
            }
        };
        let mut request = self
            .client
            .get(url)
            .header(reqwest::header::ACCEPT, "application/json");
        if let Some(token) = &self.bearer {
            request = request.bearer_auth(token);
        }
        let mut response = tokio::time::timeout(self.timeout, request.send())
            .await
            .map_err(|_| UpstreamCallError::Timeout {
                service,
                timeout_ms: self.timeout.as_millis(),
            })?
            .map_err(transport)?;
        let too_large = UpstreamCallError::ResponseTooLarge {
            service,
            limit_bytes: self.max_response_bytes,
        };
        if response
            .content_length()
            .is_some_and(|length| length > self.max_response_bytes as u64)
        {
            return Err(too_large);
        }
        let status = response.status();
        let mut body = Vec::new();
        loop {
            let chunk = tokio::time::timeout(self.timeout, response.chunk())
                .await
                .map_err(|_| UpstreamCallError::Timeout {
                    service,
                    timeout_ms: self.timeout.as_millis(),
                })?
                .map_err(transport)?;
            let Some(chunk) = chunk else { break };
            if body.len() + chunk.len() > self.max_response_bytes {
                return Err(too_large);
            }
            body.extend_from_slice(&chunk);
        }
        let parsed: Result<Value, _> = serde_json::from_slice(&body);
        if !status.is_success() {
            return Err(UpstreamCallError::Upstream {
                service,
                status: status.as_u16(),
                body: parsed.unwrap_or_else(|_| {
                    Value::String(String::from_utf8_lossy(&body).chars().take(512).collect())
                }),
            });
        }
        parsed.map_err(|error| UpstreamCallError::Transport {
            service,
            message: format!("response is not JSON: {error}"),
        })
    }
}

fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Decode `args` as the generated input type `I`, call the operation and
/// decode the body as the generated output type `O`. `prepare` may tighten
/// the typed arguments (e.g. clamp `limit`); `check` enforces result bounds
/// on the raw body before typed decoding.
pub(crate) async fn call_typed<I, O>(
    endpoint: &UpstreamEndpoint,
    method: &str,
    path_template: &str,
    output_name: &'static str,
    args: &Value,
    prepare: impl FnOnce(&mut Value),
    check: impl FnOnce(&Value) -> Result<(), UpstreamCallError>,
) -> Result<Value, UpstreamCallError>
where
    I: DeserializeOwned + Serialize,
    O: DeserializeOwned + Serialize,
{
    let service = endpoint.service();
    let args = if args.is_null() {
        Value::Object(Default::default())
    } else {
        args.clone()
    };
    let input: I =
        serde_json::from_value(args).map_err(|error| UpstreamCallError::InvalidInput {
            service,
            method: method.to_string(),
            message: error.to_string(),
        })?;
    let mut typed_args =
        serde_json::to_value(&input).map_err(|error| UpstreamCallError::InvalidInput {
            service,
            method: method.to_string(),
            message: error.to_string(),
        })?;
    prepare(&mut typed_args);
    let url = endpoint.url_for(method, path_template, &typed_args)?;
    let raw = endpoint.get_json(url).await?;
    check(&raw)?;
    let output: O = serde_json::from_value(raw).map_err(|error| UpstreamCallError::Contract {
        service,
        output: output_name,
        message: error.to_string(),
    })?;
    serde_json::to_value(output).map_err(|error| UpstreamCallError::Contract {
        service,
        output: output_name,
        message: error.to_string(),
    })
}

#[cfg(test)]
pub(crate) mod test_support {
    use serde_json::Value;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    pub(crate) fn fixture(api: &str, name: &str) -> Value {
        let path = format!(
            "{}/src/state_plugins/{api}/fixtures/{name}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(
            &std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}")),
        )
        .unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    /// `decoded` (generated type re-serialized) must carry everything the
    /// upstream body carried: numbers compare numerically, and a key may be
    /// absent only when upstream sent an empty or null value for it.
    pub(crate) fn assert_lossless(path: &str, upstream: &Value, decoded: &Value) {
        match (upstream, decoded) {
            (Value::Object(a), Value::Object(b)) => {
                for (key, value) in a {
                    match b.get(key) {
                        Some(other) => assert_lossless(&format!("{path}/{key}"), value, other),
                        None => assert!(
                            is_empty(value),
                            "{path}/{key} was dropped by the generated type: {value}"
                        ),
                    }
                }
                for key in b.keys() {
                    assert!(
                        a.contains_key(key) || is_empty(&b[key]),
                        "{path}/{key} was invented"
                    );
                }
            }
            (Value::Array(a), Value::Array(b)) => {
                assert_eq!(a.len(), b.len(), "{path} length changed");
                for (i, (x, y)) in a.iter().zip(b).enumerate() {
                    assert_lossless(&format!("{path}/{i}"), x, y);
                }
            }
            (Value::Number(a), Value::Number(b)) => {
                assert_eq!(a.as_f64(), b.as_f64(), "{path} number changed")
            }
            (a, b) => assert_eq!(a, b, "{path} changed"),
        }
    }

    fn is_empty(value: &Value) -> bool {
        match value {
            Value::Null => true,
            Value::String(s) => s.is_empty(),
            Value::Array(a) => a.is_empty(),
            Value::Object(o) => o.is_empty(),
            _ => false,
        }
    }

    /// A one-shot-per-connection HTTP/1.1 server: `respond` maps the request
    /// target (path + query) to (status, body). Returns the base URL and a
    /// channel of observed request targets.
    pub(crate) async fn serve(
        respond: impl Fn(&str) -> (u16, String) + Send + Sync + 'static,
    ) -> (String, tokio::sync::mpsc::UnboundedReceiver<String>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (seen_tx, seen_rx) = tokio::sync::mpsc::unbounded_channel();
        let respond = std::sync::Arc::new(respond);
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let respond = respond.clone();
                let seen_tx = seen_tx.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        match stream.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let head = String::from_utf8_lossy(&buf).to_string();
                    let target = head.split_whitespace().nth(1).unwrap_or("").to_string();
                    let _ = seen_tx.send(target.clone());
                    let (status, body) = respond(&target);
                    if status == 0 {
                        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                        return;
                    }
                    let response = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                });
            }
        });
        (base, seen_rx)
    }
}
