//! Tonic + tonic-web client with **server reflection** + **dynamic unary invocation**.
//!
//! - `bootstrap`: open a Channel, hit `grpc.reflection.v1.ServerReflection`,
//!   load every returned `FileDescriptorProto` into a `prost_reflect::DescriptorPool`.
//! - `invoke_unary`: JSON in → encode via DynamicMessage → unary RPC over a
//!   bytes codec → decode response DynamicMessage → JSON out.
//! - `template_for_request`: walk a method's input MessageDescriptor and emit
//!   a JSON skeleton with zero-value fields, used to seed the Explorer editor.

use anyhow::{anyhow, Context, Result};
use bytes::{Buf, BufMut};
use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage, Kind, MessageDescriptor};
use serde_json::{json, Map, Value};
use std::sync::{Arc, Mutex, RwLock};
use tokio::task::AbortHandle;
use tonic::codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};
use tonic::transport::Channel;
use tonic::{Request, Status};

/// What kind of RPC a reflected method is. The Explorer branches on this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodKind {
    Unary,
    ServerStreaming,
    ClientStreaming,
    BidiStreaming,
}

impl MethodKind {
    pub fn label(self) -> &'static str {
        match self {
            MethodKind::Unary => "unary",
            MethodKind::ServerStreaming => "server-streaming",
            MethodKind::ClientStreaming => "client-streaming",
            MethodKind::BidiStreaming => "bidi-streaming",
        }
    }
}

use tonic_reflection::pb::v1::{
    server_reflection_client::ServerReflectionClient,
    server_reflection_request::MessageRequest,
    server_reflection_response::MessageResponse,
    ServerReflectionRequest,
};

#[derive(Clone, Default)]
pub struct ReflectionRegistry {
    pool: Arc<RwLock<Option<DescriptorPool>>>,
    channel: Arc<RwLock<Option<Channel>>>,
}

impl ReflectionRegistry {
    pub fn new() -> Self { Self::default() }

    pub fn pool(&self) -> Option<DescriptorPool> {
        self.pool.read().ok().and_then(|g| g.clone())
    }
    pub fn channel(&self) -> Option<Channel> {
        self.channel.read().ok().and_then(|g| g.clone())
    }

    pub fn services(&self) -> Vec<String> {
        let pool = match self.pool() { Some(p) => p, None => return vec![] };
        let svcs: Vec<_> = pool.services().collect();
        svcs.into_iter().map(|s| s.full_name().to_string()).collect()
    }
    pub fn methods(&self, service: &str) -> Vec<String> {
        let pool = match self.pool() { Some(p) => p, None => return vec![] };
        let svcs: Vec<_> = pool.services().collect();
        svcs.into_iter()
            .find(|s| s.full_name() == service)
            .map(|s| s.methods().map(|m| m.name().to_string()).collect())
            .unwrap_or_default()
    }



    pub fn method_kind(&self, service: &str, method: &str) -> Option<MethodKind> {
        let pool = self.pool()?;
        let m = pool.services().find(|s| s.full_name() == service)?
            .methods().find(|m| m.name() == method)?;
        Some(match (m.is_client_streaming(), m.is_server_streaming()) {
            (false, false) => MethodKind::Unary,
            (false, true)  => MethodKind::ServerStreaming,
            (true,  false) => MethodKind::ClientStreaming,
            (true,  true)  => MethodKind::BidiStreaming,
        })
    }

    fn set_pool(&self, pool: DescriptorPool) {
        if let Ok(mut g) = self.pool.write() { *g = Some(pool); }
    }
    fn set_channel(&self, ch: Channel) {
        if let Ok(mut g) = self.channel.write() { *g = Some(ch); }
    }
}

// ----------------- bootstrap -----------------

pub async fn bootstrap(endpoint: &str, reg: ReflectionRegistry) -> Result<()> {
    let channel: Channel = crate::conn::connect_channel(endpoint).await?;
    reg.set_channel(channel.clone());

    let mut client = ServerReflectionClient::new(channel);
    let request = ServerReflectionRequest {
        host: "".into(),
        message_request: Some(MessageRequest::ListServices(String::new())),
    };
    let outbound = once_stream(request);
    let mut inbound = client.server_reflection_info(outbound).await?.into_inner();

    let mut pool = DescriptorPool::new();
    while let Some(msg) = inbound.message().await? {
        if let Some(MessageResponse::FileDescriptorResponse(fds)) = msg.message_response {
            for raw in fds.file_descriptor_proto {
                let fdp = prost_types::FileDescriptorProto::decode(&*raw)?;
                pool.add_file_descriptor_proto(fdp)?;
            }
        }
    }
    reg.set_pool(pool);
    Ok(())
}

// ----------------- decoding helpers -----------------

pub fn decode_to_json(reg: &ReflectionRegistry, msg_fqn: &str, bytes: &[u8]) -> Result<Value> {
    let pool = reg.pool().ok_or_else(|| anyhow!("reflection pool not loaded"))?;
    let descriptor = pool
        .get_message_by_name(msg_fqn)
        .ok_or_else(|| anyhow!("unknown message: {msg_fqn}"))?;
    let dynamic = DynamicMessage::decode(descriptor, bytes)?;
    Ok(serde_json::to_value(&dynamic)?)
}

/// Walk a request message descriptor and emit a JSON skeleton (zero values).
pub fn template_for_request(reg: &ReflectionRegistry, service: &str, method: &str) -> Result<Value> {
    let pool = reg.pool().ok_or_else(|| anyhow!("reflection pool not loaded"))?;
    let svc = pool.services().find(|s| s.full_name() == service)
        .ok_or_else(|| anyhow!("unknown service {service}"))?;
    let m = svc.methods().find(|m| m.name() == method)
        .ok_or_else(|| anyhow!("unknown method {service}/{method}"))?;
    Ok(skeleton_for(&m.input(), 0))
}

fn skeleton_for(desc: &MessageDescriptor, depth: usize) -> Value {
    if depth > 4 { return json!({}); }
    let mut obj = Map::new();
    for field in desc.fields() {
        let v = if field.is_list() {
            json!([])
        } else if field.is_map() {
            json!({})
        } else {
            match field.kind() {
                Kind::Double | Kind::Float => json!(0.0),
                Kind::Int32 | Kind::Sint32 | Kind::Sfixed32
                | Kind::Int64 | Kind::Sint64 | Kind::Sfixed64
                | Kind::Uint32 | Kind::Fixed32 | Kind::Uint64 | Kind::Fixed64 => json!(0),
                Kind::Bool => json!(false),
                Kind::String => json!(""),
                Kind::Bytes => json!(""),
                Kind::Enum(e) => json!(e.values().next().map(|v| v.name().to_string()).unwrap_or_default()),
                Kind::Message(m) => skeleton_for(&m, depth + 1),
            }
        };
        obj.insert(field.name().to_string(), v);
    }
    Value::Object(obj)
}

// ----------------- dynamic unary invocation -----------------

/// JSON in → encoded protobuf → unary RPC → JSON out.
pub async fn invoke_unary(
    reg: &ReflectionRegistry,
    service: &str,
    method: &str,
    request_json: &Value,
) -> Result<Value> {
    let pool = reg.pool().ok_or_else(|| anyhow!("reflection pool not loaded"))?;
    let channel = reg.channel().ok_or_else(|| anyhow!("no active channel"))?;

    let svc = pool.services().find(|s| s.full_name() == service)
        .ok_or_else(|| anyhow!("unknown service {service}"))?;
    let m = svc.methods().find(|m| m.name() == method)
        .ok_or_else(|| anyhow!("unknown method {service}/{method}"))?;
    if m.is_client_streaming() || m.is_server_streaming() {
        return Err(anyhow!("only unary methods are supported by the Explorer"));
    }

    let input_desc = m.input();
    let output_desc = m.output();
    let req_msg = DynamicMessage::deserialize(input_desc, request_json)
        .context("request JSON does not match input schema")?;
    let req_bytes = req_msg.encode_to_vec();

    let path = format!("/{}/{}", service, method);
    let mut grpc = tonic::client::Grpc::new(channel);
    grpc.ready().await.map_err(|e| anyhow!("channel not ready: {e}"))?;
    let resp = grpc
        .unary(Request::new(req_bytes), path.parse()?, BytesCodec)
        .await
        .map_err(|s: Status| anyhow!("gRPC {}: {}", s.code(), s.message()))?;

    let resp_bytes = resp.into_inner();
    let resp_msg = DynamicMessage::decode(output_desc, resp_bytes.as_slice())?;
    Ok(serde_json::to_value(&resp_msg)?)
}

// ----------------- BytesCodec: passthrough Vec<u8> in/out -----------------

#[derive(Default, Clone, Copy)]
pub struct BytesCodec;

impl Codec for BytesCodec {
    type Encode = Vec<u8>;
    type Decode = Vec<u8>;
    type Encoder = BytesCodec;
    type Decoder = BytesCodec;
    fn encoder(&mut self) -> Self::Encoder { *self }
    fn decoder(&mut self) -> Self::Decoder { *self }
}

impl Encoder for BytesCodec {
    type Item = Vec<u8>;
    type Error = Status;
    fn encode(&mut self, item: Self::Item, dst: &mut EncodeBuf<'_>) -> Result<(), Self::Error> {
        dst.put_slice(&item);
        Ok(())
    }
}

impl Decoder for BytesCodec {
    type Item = Vec<u8>;
    type Error = Status;
    fn decode(&mut self, src: &mut DecodeBuf<'_>) -> Result<Option<Self::Item>, Self::Error> {
        let n = src.remaining();
        let mut out = vec![0u8; n];
        src.copy_to_slice(&mut out);
        Ok(Some(out))
    }
}

// ----------------- background invocation handle -----------------

/// Stored on the app; the UI polls it once per frame.
#[derive(Clone, Default)]
pub struct InvokeHandle {
    inner: Arc<Mutex<InvokeState>>,
}

#[derive(Default)]
struct InvokeState {
    in_flight: bool,
    result: Option<Result<Value, String>>,
}

impl InvokeHandle {
    pub fn new() -> Self { Self::default() }

    pub fn in_flight(&self) -> bool {
        self.inner.lock().map(|g| g.in_flight).unwrap_or(false)
    }

    pub fn take_result(&self) -> Option<Result<Value, String>> {
        self.inner.lock().ok().and_then(|mut g| g.result.take())
    }

    pub fn spawn(
        &self,
        reg: ReflectionRegistry,
        service: String,
        method: String,
        body: Value,
        ctx: egui::Context,
    ) {
        {
            let mut g = self.inner.lock().unwrap();
            if g.in_flight { return; }
            g.in_flight = true;
            g.result = None;
        }
        let inner = self.inner.clone();
        tokio::spawn(async move {
            let res = invoke_unary(&reg, &service, &method, &body).await
                .map_err(|e| format!("{e:#}"));
            if let Ok(mut g) = inner.lock() {
                g.in_flight = false;
                g.result = Some(res);
            }
            ctx.request_repaint();
        });
    }
}

// ----------------- request validation -----------------

/// Validate raw JSON text against a method's input descriptor.
/// Returns `Ok(())` if the JSON parses AND deserialises into a DynamicMessage
/// of the method's input type. Used for live editor feedback.
pub fn validate_request(
    reg: &ReflectionRegistry,
    service: &str,
    method: &str,
    raw: &str,
) -> Result<()> {
    let pool = reg.pool().ok_or_else(|| anyhow!("reflection pool not loaded"))?;
    let svc = pool.services().find(|s| s.full_name() == service)
        .ok_or_else(|| anyhow!("unknown service {service}"))?;
    let m = svc.methods().find(|m| m.name() == method)
        .ok_or_else(|| anyhow!("unknown method {service}/{method}"))?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("request body is empty"));
    }
    let value: Value = serde_json::from_str(trimmed)
        .map_err(|e| anyhow!("invalid JSON at line {} col {}: {}", e.line(), e.column(), e))?;
    DynamicMessage::deserialize(m.input(), value)
        .context("JSON does not match input message schema")?;
    Ok(())
}

// ----------------- server-streaming invocation -----------------

/// Shared, frame-polled state for a single server-streaming RPC.
#[derive(Clone, Default)]
pub struct StreamHandle {
    inner: Arc<Mutex<StreamState>>,
}

#[derive(Default)]
struct StreamState {
    running: bool,
    items: Vec<Value>,
    error: Option<String>,
    closed: bool,
    abort: Option<AbortHandle>,
}

#[derive(Clone, Debug)]
pub struct StreamSnapshot {
    pub running: bool,
    pub closed: bool,
    pub error: Option<String>,
    pub items: Vec<Value>,
}

impl StreamHandle {
    pub fn new() -> Self { Self::default() }

    pub fn snapshot(&self) -> StreamSnapshot {
        let g = self.inner.lock().unwrap();
        StreamSnapshot {
            running: g.running,
            closed: g.closed,
            error: g.error.clone(),
            items: g.items.clone(),
        }
    }

    pub fn clear(&self) {
        if let Ok(mut g) = self.inner.lock() {
            if let Some(a) = g.abort.take() { a.abort(); }
            *g = StreamState::default();
        }
    }

    pub fn cancel(&self) {
        if let Ok(mut g) = self.inner.lock() {
            if let Some(a) = g.abort.take() { a.abort(); }
            g.running = false;
            g.closed = true;
        }
    }

    pub fn spawn(
        &self,
        reg: ReflectionRegistry,
        service: String,
        method: String,
        body: Value,
        ctx: egui::Context,
    ) {
        {
            let mut g = self.inner.lock().unwrap();
            if g.running { return; }
            if let Some(a) = g.abort.take() { a.abort(); }
            *g = StreamState { running: true, ..Default::default() };
        }
        let inner = self.inner.clone();
        let handle = tokio::spawn(async move {
            let res = run_server_streaming(reg, service, method, body, inner.clone(), ctx.clone()).await;
            if let Ok(mut g) = inner.lock() {
                g.running = false;
                g.closed = true;
                if let Err(e) = res {
                    g.error = Some(format!("{e:#}"));
                }
            }
            ctx.request_repaint();
        });
        if let Ok(mut g) = self.inner.lock() {
            g.abort = Some(handle.abort_handle());
        }
    }
}

async fn run_server_streaming(
    reg: ReflectionRegistry,
    service: String,
    method: String,
    body: Value,
    inner: Arc<Mutex<StreamState>>,
    ctx: egui::Context,
) -> Result<()> {
    let pool = reg.pool().ok_or_else(|| anyhow!("reflection pool not loaded"))?;
    let channel = reg.channel().ok_or_else(|| anyhow!("no active channel"))?;

    let svc = pool.services().find(|s| s.full_name() == service)
        .ok_or_else(|| anyhow!("unknown service {service}"))?;
    let m = svc.methods().find(|m| m.name() == method)
        .ok_or_else(|| anyhow!("unknown method {service}/{method}"))?;
    if !m.is_server_streaming() || m.is_client_streaming() {
        return Err(anyhow!("method is not server-streaming"));
    }

    let input_desc = m.input();
    let output_desc = m.output();
    let req_msg = DynamicMessage::deserialize(input_desc, &body)
        .context("request JSON does not match input schema")?;
    let req_bytes = req_msg.encode_to_vec();

    let path = format!("/{}/{}", service, method);
    let mut grpc = tonic::client::Grpc::new(channel);
    grpc.ready().await.map_err(|e| anyhow!("channel not ready: {e}"))?;
    let resp = grpc
        .server_streaming(Request::new(req_bytes), path.parse()?, BytesCodec)
        .await
        .map_err(|s: Status| anyhow!("gRPC {}: {}", s.code(), s.message()))?;

    let mut stream = resp.into_inner();
    loop {
        match stream.message().await {
            Ok(Some(bytes)) => {
                let msg = DynamicMessage::decode(output_desc.clone(), bytes.as_slice())
                    .context("failed to decode streamed item")?;
                let val = serde_json::to_value(&msg)?;
                if let Ok(mut g) = inner.lock() {
                    g.items.push(val);
                }
                ctx.request_repaint();
            }
            Ok(None) => break,
            Err(s) => return Err(anyhow!("gRPC {}: {}", s.code(), s.message())),
        }
    }
    Ok(())
}

// ----------------- internal -----------------

fn once_stream<T: Send + 'static>(
    item: T,
) -> impl tonic::codegen::tokio_stream::Stream<Item = T> + Send + 'static {
    use tonic::codegen::tokio_stream::wrappers::ReceiverStream;
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(async move { let _ = tx.send(item).await; });
    ReceiverStream::new(rx)
}

