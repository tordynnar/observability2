# OpenTelemetry + gRPC in Rust

## Table of Contents

1. [Dependencies](#dependencies)
2. [How It Works](#how-it-works)
3. [Telemetry Initialization](#telemetry-initialization)
4. [Example Code](#example-code)
5. [Proto Compilation](#proto-compilation)
6. [Launch Scripts](#launch-scripts)
7. [Environment Variables](#environment-variables)
8. [Design Decisions](#design-decisions)
9. [Testing That It Works](#testing-that-it-works)
10. [Troubleshooting](#troubleshooting)

---

## Dependencies

### Cargo.toml

```toml
[dependencies]
tonic = "0.14"
tonic-prost = "0.14"
prost = "0.14"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "signal"] }
opentelemetry = { version = "0.31", features = ["trace", "logs"] }
opentelemetry_sdk = { version = "0.31", features = ["rt-tokio"] }
opentelemetry-stdout = { version = "0.31", features = ["trace", "logs"] }
opentelemetry-otlp = { version = "0.31", features = ["trace", "logs", "grpc-tonic"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "registry"] }
tracing-opentelemetry = "0.32"
opentelemetry-appender-tracing = "0.31"
tonic-tracing-opentelemetry = { version = "0.32", features = ["tracing_level_info"] }
tower = "0.5"

[build-dependencies]
tonic-build = "0.14"
tonic-prost-build = "0.14"
```

---

## How It Works

Rust uses the **`tracing` crate** as a unified diagnostics layer. Unlike Go (separate `slog` and OTel APIs) or Python (separate `logging` and `opentelemetry`), `tracing` serves as the single entry point for both structured logging and span creation. Application code only calls `tracing::info!()` and `tracing::info_span!()` -- it never touches the OTel API directly.

The `tracing` ecosystem uses a **subscriber pattern**: a global subscriber composed of multiple layers, each independently processing every span and event. The subscriber has three layers:

1. **`fmt::Layer`** -- human-readable log lines to stderr (controlled by `RUST_LOG`)
2. **`OpenTelemetryLayer`** (from `tracing-opentelemetry`) -- converts `tracing::Span` into OTel spans, exported via `SdkTracerProvider`
3. **`OpenTelemetryTracingBridge`** (from `opentelemetry-appender-tracing`) -- converts `tracing::info!()` events into OTel `LogRecord`, exported via `SdkLoggerProvider`

A single `tracing::info!()` call simultaneously produces: a stderr log line, an OTel span event, and an OTel log record.

### Automatic context propagation

`tonic-tracing-opentelemetry` provides tower middleware layers:

- **Server `OtelGrpcLayer`** -- extracts `traceparent` from incoming HTTP headers, creates a server span with RPC attributes, sets extracted context as parent.
- **Client `OtelGrpcLayer`** -- creates a client span with RPC attributes, injects `traceparent` into outgoing HTTP headers.

These are the Rust equivalent of Go's `otelgrpc.NewServerHandler()`/`NewClientHandler()`. Context is implicit: `tracing` maintains a task-local span stack, so `tracing::info!()` inside a middleware-created span automatically inherits the trace context.

---

## Telemetry Initialization

```rust
use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub struct TelemetryGuard {
    tracer_provider: SdkTracerProvider,
    logger_provider: SdkLoggerProvider,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        let _ = self.tracer_provider.shutdown();
        let _ = self.logger_provider.shutdown();
    }
}

pub fn init() -> TelemetryGuard {
    let resource = Resource::builder().build();

    let traces_exporter = std::env::var("OTEL_TRACES_EXPORTER").unwrap_or_default();
    let mut tracer_builder = SdkTracerProvider::builder().with_resource(resource.clone());
    tracer_builder = match traces_exporter.as_str() {
        "console" => {
            tracer_builder.with_batch_exporter(opentelemetry_stdout::SpanExporter::default())
        }
        "otlp" => tracer_builder.with_batch_exporter(
            opentelemetry_otlp::SpanExporter::builder()
                .with_tonic()
                .build()
                .expect("failed to build OTLP span exporter"),
        ),
        _ => tracer_builder,
    };
    let tracer_provider = tracer_builder.build();
    let tracer = tracer_provider.tracer("app");
    global::set_tracer_provider(tracer_provider.clone());

    let logs_exporter = std::env::var("OTEL_LOGS_EXPORTER").unwrap_or_default();
    let mut logger_builder = SdkLoggerProvider::builder().with_resource(resource);
    logger_builder = match logs_exporter.as_str() {
        "console" => {
            logger_builder.with_batch_exporter(opentelemetry_stdout::LogExporter::default())
        }
        "otlp" => logger_builder.with_batch_exporter(
            opentelemetry_otlp::LogExporter::builder()
                .with_tonic()
                .build()
                .expect("failed to build OTLP log exporter"),
        ),
        _ => logger_builder,
    };
    let logger_provider = logger_builder.build();

    global::set_text_map_propagator(TraceContextPropagator::new());

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(
            &logger_provider,
        ))
        .init();

    TelemetryGuard {
        tracer_provider,
        logger_provider,
    }
}
```

---

## Example Code

```rust
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tower::ServiceBuilder;

use example::{pb, telemetry};

struct GreeterService;

#[tonic::async_trait]
impl pb::greeter_server::Greeter for GreeterService {
    async fn say_hello(
        &self,
        request: Request<pb::HelloRequest>,
    ) -> Result<Response<pb::HelloReply>, Status> {
        let name = request.into_inner().name;
        tracing::info!(name = %name, "Received request");
        Ok(Response::new(pb::HelloReply {
            message: format!("Hello, {}!", name),
        }))
    }
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for ctrl+c");
    tracing::info!("Shutting down server");
}

async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "[::]:50051".parse()?;
    tracing::info!("Server starting on port 50051");

    Server::builder()
        .layer(tonic_tracing_opentelemetry::middleware::server::OtelGrpcLayer::default())
        .add_service(pb::greeter_server::GreeterServer::new(GreeterService))
        .serve_with_shutdown(addr, shutdown_signal())
        .await?;

    Ok(())
}

async fn call() -> Result<(), Box<dyn std::error::Error>> {
    let channel = tonic::transport::Channel::from_static("http://localhost:50051")
        .connect()
        .await?;
    let channel = ServiceBuilder::new()
        .layer(tonic_tracing_opentelemetry::middleware::client::OtelGrpcLayer)
        .service(channel);

    let mut client = pb::greeter_client::GreeterClient::new(channel);

    let response = client
        .say_hello(Request::new(pb::HelloRequest {
            name: "World".into(),
        }))
        .await?;

    tracing::info!(message = %response.into_inner().message, "Greeter response");

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let guard = telemetry::init();

    serve().await?; // or call().await?

    drop(guard);
    Ok(())
}
```

Key points:
- **`OtelGrpcLayer::default()`** -- server-side tower middleware. Extracts `traceparent` from incoming metadata, creates a server span with RPC attributes, enters the span for the handler's duration.
- **`ServiceBuilder::new().layer(OtelGrpcLayer).service(channel)`** -- wraps the raw tonic channel with client-side OTel middleware. Creates client spans and injects `traceparent`.
- **`drop(guard)`** -- explicit flush at the end of `main()`. Critical for ensuring all buffered telemetry is exported before process exit.

---

## Proto Compilation

Rust compiles protos at build time via `build.rs` -- no generated files are committed:

### build.rs

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_prost_build::compile_protos("proto/helloworld.proto")?;
    Ok(())
}
```

### lib.rs

```rust
pub mod telemetry;

pub mod pb {
    tonic::include_proto!("helloworld");
}
```

The `include_proto!` macro includes the code generated by `build.rs`. The `pb` module contains `HelloRequest`, `HelloReply`, `greeter_server::Greeter` trait, `greeter_server::GreeterServer`, and `greeter_client::GreeterClient`.

This requires `protoc` to be installed. On macOS: `brew install protobuf`. On Debian/Ubuntu: `apt install protobuf-compiler`.

---

## Launch Scripts

```bash
export OTEL_SERVICE_NAME="example1"  # or "example2"
export OTEL_TRACES_EXPORTER="console"
export OTEL_LOGS_EXPORTER="console"
export OTEL_PROPAGATORS="tracecontext,baggage"
export OTEL_BSP_SCHEDULE_DELAY="1"
export OTEL_BLRP_SCHEDULE_DELAY="1"
export RUST_LOG="info"

cargo run
```

---

## Environment Variables

See SKILL.md for common `OTEL_*` environment variables. The one Rust-specific variable:

### `RUST_LOG` is critical

If `RUST_LOG` is not set, the `EnvFilter` defaults to `error` only. No INFO-level events will be emitted, which means no spans from the middleware (set to INFO level via `tracing_level_info` feature) and no application logs. Always set `RUST_LOG=info` at minimum.

---

## Design Decisions

### 1. `tracing` with layered subscriber over raw OTel API

Rust's `tracing` crate is the de facto structured diagnostics standard. Using it decouples application code from OTel -- the code uses `info!()` and `info_span!()`, and the OTel bridge layer handles export. The `tracing-subscriber` registry composes fmt (stderr), OTel traces, and OTel logs in one subscriber, so a single `tracing::info!()` call simultaneously produces a stderr log line, an OTel span event, and an OTel log record.

### 2. `tonic` for gRPC

The dominant Rust gRPC framework, built on tower/hyper/prost. Tower's composable middleware (service + layer) model makes instrumentation pluggable -- the `OtelGrpcLayer` is just another layer in the tower stack. Async-first, integrates naturally with tokio.

### 3. `build.rs` for proto compilation

Rust idiom: compile protos at build time, never commit generated code. Unlike Go and Python (which commit generated files), `tonic-prost-build` integrates with Cargo's build system. The tradeoff is requiring `protoc` on the build machine.

### 4. `tonic-tracing-opentelemetry` for automatic instrumentation

Provides tower middleware that auto-creates spans and propagates trace context. Eliminates manual `Injector`/`Extractor` code. The `tracing_level_info` feature is enabled so middleware spans are at INFO level -- without it, the default TRACE level would be filtered out by `RUST_LOG=info`, and you'd see no gRPC spans.

---

## Testing That It Works

Rust-specific output format -- **stderr** (human-readable via `fmt::Layer`):
```
2026-02-21T12:03:30Z  INFO helloworld.Greeter/SayHello{otel.kind="server" rpc.system="grpc" ...}: server: Received request name=World
```

**stdout** (console exporter spans):
```
Spans
Resource
     ->  service.name=String(Owned("grpc-server"))
Span #0
    Name         : helloworld.Greeter/SayHello
    TraceId      : 8267a68f2f0b720634bb2d30d0710acb
    SpanId       : f7a0c0538352f217
    ParentSpanId : 5d2a1bf665bbc02b
    Kind         : Server
    Attributes:
         ->  rpc.system: String(Owned("grpc"))
         ->  rpc.service: String(Owned("helloworld.Greeter"))
         ->  rpc.method: String(Owned("SayHello"))
```

**stdout** (console exporter logs):
```
Logs
Log #0
    TraceId: 8267a68f2f0b720634bb2d30d0710acb
    SpanId: f7a0c0538352f217
    SeverityText: "INFO"
    Body: String(Owned("Received request"))
    Attributes:
         ->  name: String(Owned("World"))
```

---

## Troubleshooting

### 1. No output at all (no logs, no spans)

**Symptom:** Running the server or client produces no visible output.

**Cause:** `RUST_LOG` is not set. The `EnvFilter` defaults to `error` only, filtering out all INFO-level events.

**Fix:** Set `RUST_LOG=info` in your launch script:
```bash
export RUST_LOG="info"
```

For less noise from framework internals: `RUST_LOG=info,hyper=warn,tower=warn`

### 2. `cargo build` fails with protoc error

**Symptom:** Build error mentioning `protoc` or `tonic-prost-build`.

**Cause:** `protoc` is not installed. Rust compiles protos at build time.

**Fix:**
```bash
# macOS:
brew install protobuf
# Debian/Ubuntu:
apt install protobuf-compiler
```

### 3. "unresolved module" error for `tonic_prost`

**Symptom:** Compilation error: `use of undeclared crate or module tonic_prost`.

**Cause:** `tonic-prost` (the runtime crate) is missing from `[dependencies]`. The generated code references `tonic_prost::ProstCodec`. Easy to miss because the build dependency `tonic-prost-build` and the runtime dependency `tonic-prost` are separate crates.

**Fix:** Add to `Cargo.toml`:
```toml
tonic-prost = "0.14"
```

### 4. TelemetryGuard dropped too early / client exits without exporting

**Symptom:** No spans or log records appear, even though `RUST_LOG=info` is set and the middleware is attached. Especially common with short-lived clients.

**Cause:** The `TelemetryGuard` was dropped prematurely (shutting down both providers), or the process exited before the batch processor flushed. Common mistakes:
```rust
let _ = telemetry::init();  // guard dropped immediately!
telemetry::init();           // return value discarded
```

**Fix:** Bind the guard and call `drop(guard)` explicitly at the end of `main()`. This triggers `shutdown()` which forces a flush:
```rust
let guard = telemetry::init();
// ... application runs ...
drop(guard);  // explicit drop at the end
```

### 5. Client and server spans have different trace IDs

**Symptom:** Both produce spans, but `TraceId` values don't match.

**Cause:** Client-side `OtelGrpcLayer` is not attached. The client channel must be wrapped:
```rust
let channel = ServiceBuilder::new()
    .layer(tonic_tracing_opentelemetry::middleware::client::OtelGrpcLayer)
    .service(channel);
```

Without this, no `traceparent` header is injected into outgoing metadata.

### 6. Spans appear but no log records

**Symptom:** OTel spans appear on stdout, but no `Logs` section.

**Cause:** `OTEL_LOGS_EXPORTER` is not set, or `OpenTelemetryTracingBridge` is not in the subscriber layers.

**Fix:** Verify both: `OTEL_LOGS_EXPORTER=console` in the launch script, and the bridge layer is included in `init()`:
```rust
.with(opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(
    &logger_provider,
))
```

### 7. gRPC middleware spans not appearing

**Symptom:** Application `tracing::info!()` events appear, but no gRPC span (no `helloworld.Greeter/SayHello` span).

**Cause:** The `tracing_level_info` feature is not enabled on `tonic-tracing-opentelemetry`. By default, middleware spans are at TRACE level, which is filtered out by `RUST_LOG=info`.

**Fix:** Add the feature in `Cargo.toml`:
```toml
tonic-tracing-opentelemetry = { version = "0.32", features = ["tracing_level_info"] }
```

