# gRPC + OpenTelemetry in Rust: A Complete Guide

This project demonstrates distributed tracing and log/trace correlation for a Rust gRPC application using OpenTelemetry. All telemetry is exported to the console so you can see exactly what the SDK produces.

## What This Project Does

A gRPC `Greeter` service with a single `SayHello` RPC. A client sends a request; the server responds. OpenTelemetry instruments both sides, producing:

1. **Distributed traces** — a CLIENT span on the caller, a SERVER span on the callee, linked by W3C Trace Context propagated through gRPC metadata.
2. **OTel log records** — `tracing` events bridged into the OpenTelemetry Logs pipeline and exported as structured output to stdout, with trace_id/span_id for correlation.

## Project Structure

```
rust/
├── proto/
│   └── helloworld.proto              # gRPC service definition (no language-specific options)
├── src/
│   ├── lib.rs                        # Exports telemetry module + proto-generated code
│   ├── telemetry.rs                  # OTel initialization (providers, exporters, tracing subscriber)
│   └── bin/
│       ├── server.rs                 # gRPC server with OTel instrumentation
│       └── client.rs                 # gRPC client with OTel instrumentation
├── build.rs                          # Proto compilation via tonic-prost-build
├── Cargo.toml                        # Dependencies
├── run_server.sh                     # Launch script with OTEL_* env vars
├── run_client.sh                     # Launch script with OTEL_* env vars
└── README.md
```

## Quick Start

```bash
cd rust

# Terminal 1: start the server
chmod +x run_server.sh run_client.sh
./run_server.sh

# Terminal 2: send a request
./run_client.sh
```

No proto generation step needed — `tonic-prost-build` compiles the `.proto` file at build time via `build.rs`. This requires `protoc` to be installed. On macOS: `brew install protobuf`.

---

## Dependencies

Requires a Rust toolchain (edition 2021+) and `protoc`. Dependencies are managed by Cargo (`Cargo.toml`).

### gRPC

| Crate | Why |
|---|---|
| `tonic` | The gRPC framework for Rust, built on tower/hyper. Provides `Server`, `Channel`, codegen macros. |
| `tonic-prost` | Runtime codec that connects tonic to prost for message serialization. The generated code references `tonic_prost::ProstCodec`. |
| `tonic-prost-build` | Build-time proto compiler. Used in `build.rs` to generate Rust types and gRPC service traits from `.proto` files. |
| `prost` | Protocol Buffers runtime for Rust. The generated message types depend on it. |
| `tokio` | Async runtime. Tonic requires tokio's multi-threaded runtime. Also provides `signal::ctrl_c()` for graceful shutdown. |

### OpenTelemetry Core

| Crate | Why |
|---|---|
| `opentelemetry` | The public API surface: `global::set_tracer_provider()`, `global::set_text_map_propagator()`. By itself it delegates to no-op implementations until an SDK is registered. |
| `opentelemetry_sdk` | The concrete SDK: `SdkTracerProvider`, `BatchSpanProcessor`, `SdkLoggerProvider`. This is what actually records spans and log records and exports them. |
| `opentelemetry-stdout` | Exports spans and log records as structured output to stdout. The development-time equivalent of an OTLP exporter. |

### Bridges: `tracing` ↔ OpenTelemetry

| Crate | Why |
|---|---|
| `tracing` | Rust's de facto structured diagnostics crate. Provides `info!`, `info_span!`, the `Instrument` trait, and the subscriber/layer pattern. All application logging and span creation goes through `tracing`. |
| `tracing-subscriber` | Composable subscriber layers. We use `fmt::Layer` (human-readable stderr), `EnvFilter` (controls log levels via `RUST_LOG`), and the `Registry` that holds all layers. |
| `tracing-opentelemetry` | Bridges `tracing` spans → OTel spans. The `OpenTelemetryLayer` converts every `tracing::Span` into an OTel span, exported via the `SdkTracerProvider`. |
| `opentelemetry-appender-tracing` | Bridges `tracing` events → OTel log records. The `OpenTelemetryTracingBridge` converts every `tracing::info!()` call into an OTel `LogRecord`, exported via the `SdkLoggerProvider`. |

### gRPC Instrumentation

| Crate | Why |
|---|---|
| `tonic-tracing-opentelemetry` | Tower middleware layers for tonic that automatically create spans and propagate trace context for every RPC — the Rust equivalent of Go's `otelgrpc.NewServerHandler()`/`NewClientHandler()`. The server layer extracts `traceparent` from incoming metadata and creates a server span; the client layer creates a client span and injects `traceparent` into outgoing metadata. |
| `tower` | Composable middleware framework. Used with `ServiceBuilder` to wrap tonic channels with the OTel client layer. |

---

## How It Works

### Explicit Initialization, Automatic Instrumentation

Like Go, Rust has no `initialize()` magic call or monkey-patching. The OTel SDK is wired explicitly in `telemetry::init()`. But like Go's `otelgrpc` stats handlers, gRPC instrumentation is automatic via tower middleware layers:

```rust
// src/telemetry.rs — called once at startup
let guard = telemetry::init();

// src/bin/server.rs — OtelGrpcLayer auto-creates spans and extracts trace context
Server::builder()
    .layer(OtelGrpcLayer::default())
    .add_service(GreeterServer::new(greeter))
    .serve_with_shutdown(addr, shutdown_signal())
    .await?;

// src/bin/client.rs — OtelGrpcLayer auto-creates spans and injects trace context
let channel = ServiceBuilder::new()
    .layer(OtelGrpcLayer)
    .service(channel);
let mut client = GreeterClient::new(channel);
```

`telemetry::init()` does the following:

1. **Creates a `Resource`** via `Resource::builder().build()`, which includes the built-in `SdkProvidedResourceDetector` and `EnvResourceDetector` — these read `OTEL_SERVICE_NAME` and `OTEL_RESOURCE_ATTRIBUTES` from the environment automatically.
2. **Creates a `SdkTracerProvider`** — reads `OTEL_TRACES_EXPORTER` and attaches a `BatchSpanProcessor` with the selected exporter (`console`, `otlp`, or none).
3. **Creates a `SdkLoggerProvider`** — reads `OTEL_LOGS_EXPORTER` and attaches a batch exporter with the selected exporter (`console`, `otlp`, or none).
4. **Sets the global propagator** to W3C `TraceContextPropagator`.
5. **Builds a layered `tracing` subscriber** with three layers (see below).
6. **Returns a `TelemetryGuard`** that flushes and shuts down both providers when dropped.

### The `tracing` Crate: Rust's Unified Diagnostics Layer

Unlike Go (which has separate `log/slog` and `otel` APIs) or Python (which has separate `logging` and `opentelemetry` packages), Rust's `tracing` crate serves as the single entry point for both structured logging and span creation. Application code only calls `tracing::info!()` and `tracing::info_span!()` — it never touches the OpenTelemetry API directly.

The `tracing` ecosystem uses a **subscriber pattern**: you register a global subscriber composed of multiple layers, and each layer independently processes every span and event. Our subscriber has three layers:

1. **`fmt::Layer`** — writes human-readable log lines to stderr. This is what you see during development.
2. **`OpenTelemetryLayer`** (from `tracing-opentelemetry`) — converts every `tracing::Span` into an OTel span, which gets exported as structured output to stdout via the `SdkTracerProvider`.
3. **`OpenTelemetryTracingBridge`** (from `opentelemetry-appender-tracing`) — converts every `tracing::info!()` event into an OTel `LogRecord`, which gets exported to stdout via the `SdkLoggerProvider`.

### Implicit Span Context for Logs

Unlike Go where you must call `slog.InfoContext(ctx, ...)` to correlate logs with traces, in Rust `tracing::info!()` inside a middleware-created span **automatically** associates with the parent span. The `OpenTelemetryTracingBridge` picks up the trace context without explicit context passing:

```rust
// Go — context passing is explicit:
slog.InfoContext(ctx, "Received request", "name", req.GetName())

// Rust — context is implicit via the middleware's span:
async fn say_hello(&self, request: Request<HelloRequest>) -> ... {
    tracing::info!(name = %name, "Received request");
    // ^^^ automatically gets trace_id/span_id from the OtelGrpcLayer span
}
```

This is because `tracing` uses a thread-local (or task-local in async) span stack. The `OtelGrpcLayer` enters a span for the duration of each RPC, and any `tracing::info!()` call within the handler automatically inherits the span context.

### Automatic Context Propagation via `tonic-tracing-opentelemetry`

The `tonic-tracing-opentelemetry` crate provides tower middleware layers that are the Rust equivalent of Go's `otelgrpc.NewServerHandler()`/`NewClientHandler()`:

- **Server `OtelGrpcLayer`** — extracts `traceparent` from incoming HTTP headers, creates a server span with RPC attributes (`rpc.system`, `rpc.service`, `rpc.method`), and sets the extracted context as the span's parent.
- **Client `OtelGrpcLayer`** — creates a client span with RPC attributes and injects `traceparent` into outgoing HTTP headers.

Both layers also record `rpc.grpc.status_code` and `otel.status_code` from the response.

---

## Environment Variables

The launch scripts set these environment variables.

| Variable | Value | Read by |
|---|---|---|
| `OTEL_SERVICE_NAME` | `grpc-server` / `grpc-client` | `Resource::builder()` includes `SdkProvidedResourceDetector`, which reads this env var automatically and sets `service.name` on the resource attached to all spans and log records. Falls back to `OTEL_RESOURCE_ATTRIBUTES`, then `"unknown_service"`. |
| `OTEL_TRACES_EXPORTER` | `console` | Read by `telemetry::init()` to select the span exporter: `console` (stdout), `otlp` (gRPC to collector), or `none`/unset (no exporter). |
| `OTEL_LOGS_EXPORTER` | `console` | Read by `telemetry::init()` to select the log exporter: `console` (stdout), `otlp` (gRPC to collector), or `none`/unset (no exporter). |
| `OTEL_PROPAGATORS` | `tracecontext,baggage` | Not read by code (W3C TraceContext is hardcoded). Set for documentation consistency. |
| `OTEL_BSP_SCHEDULE_DELAY` | `1` | `BatchSpanProcessor` schedule delay in milliseconds (default: 5000). Read by the SDK's `BatchConfigBuilder` via `init_from_env_vars()`. Setting to `1` means spans appear within ~1ms of completion. |
| `OTEL_BLRP_SCHEDULE_DELAY` | `1` | `BatchLogProcessor` schedule delay in milliseconds (default: 1000). Read by the SDK's `BatchConfigBuilder` via `init_from_env_vars()`. Setting to `1` means log records appear immediately. |
| `RUST_LOG` | `info` | `tracing_subscriber::EnvFilter` — controls which `tracing` events are emitted. Supports per-module filtering (e.g., `RUST_LOG=info,hyper=warn`). |

---

## Exporting via OTLP Instead of Console

The stdout exporter is useful for seeing raw telemetry during development, but in a real setup you'll send telemetry to a collector or backend via OTLP. No code changes are needed — just set the environment variables:

```bash
export OTEL_TRACES_EXPORTER=otlp
export OTEL_LOGS_EXPORTER=otlp
export OTEL_EXPORTER_OTLP_ENDPOINT="http://localhost:4317"
```

### OTLP Configuration Variables

| Variable | Default | What It Does |
|---|---|---|
| `OTEL_EXPORTER_OTLP_ENDPOINT` | `http://localhost:4317` (gRPC) | The collector endpoint. |
| `OTEL_EXPORTER_OTLP_PROTOCOL` | `grpc` | Protocol to use. |
| `OTEL_EXPORTER_OTLP_HEADERS` | *(none)* | Comma-separated `key=value` pairs for authentication. |

---

## How Distributed Tracing Works Across gRPC

Here's the exact flow when the client calls `SayHello`:

### 1. Client creates a span and injects context

The client's `OtelGrpcLayer` intercepts the outgoing request. It creates a `tracing::Span` with `SpanKind=Client` and RPC attributes. It then injects the span's OTel context into the HTTP headers as a `traceparent`:
```
traceparent: 00-c394bed697951b977323b0c1a717497c-0a8c1b6cfa268826-01
```

### 2. Server extracts the context and creates a child span

The server's `OtelGrpcLayer` intercepts the incoming request. It extracts the `traceparent` header and reconstructs the remote `SpanContext`, then creates a span with `SpanKind=Server` and sets the extracted context as the parent. The server span gets the **same TraceId** as the client span, and its ParentSpanId is the client span's SpanId.

### 3. Log correlation happens automatically

Inside the middleware's span, `tracing::info!()` events in the handler automatically carry the span's trace context. The `OpenTelemetryTracingBridge` converts these into OTel `LogRecord`s with `trace_id` and `span_id` fields.

### 4. Verification: matching IDs

```
# Client span (stdout):
TraceId      : c394bed697951b977323b0c1a717497c
SpanId       : 0a8c1b6cfa268826
ParentSpanId : None (root span)
Kind         : Client

# Server span (stdout):
TraceId      : c394bed697951b977323b0c1a717497c    # same trace!
SpanId       : 1a46c54888583b4f
ParentSpanId : 0a8c1b6cfa268826                      # matches client SpanId
Kind         : Server

# Server OTel log record (stdout):
TraceId: c394bed697951b977323b0c1a717497c            # same trace as both spans
SpanId: 1a46c54888583b4f                              # matches server span
```

---

## Two Kinds of Telemetry Output

### 1. Human-readable logs to stderr

Standard `tracing` fmt layer output, showing span context:

```
2026-02-21T12:03:30Z  INFO helloworld.Greeter/SayHello{otel.kind="server" rpc.system="grpc" ...}: server: Received request name=World
```

Controlled by: `RUST_LOG=info`

### 2. OTel spans and log records to stdout

`opentelemetry-stdout` output — structured span and log record exports:

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

Logs
Log #0
    TraceId: 8267a68f2f0b720634bb2d30d0710acb
    SpanId: f7a0c0538352f217
    SeverityText: "INFO"
    Body: String(Owned("Received request"))
    Attributes:
         ->  name: String(Owned("World"))
```

Controlled by: `OTEL_TRACES_EXPORTER` and `OTEL_LOGS_EXPORTER` (set to `console` in run scripts)

---

## Rust vs Go vs Python

| Aspect | Python | Go | Rust |
|---|---|---|---|
| **Initialization** | `initialize()` — discovers and activates instrumentors + SDK via entry points and env vars | Explicit: create providers, set globals, pass stats handlers | Explicit: create providers, build layered `tracing` subscriber |
| **gRPC instrumentation** | Monkey-patching via `GrpcInstrumentorServer/Client` | Stats handlers: `otelgrpc.NewServerHandler()`/`NewClientHandler()` | Tower middleware: `tonic-tracing-opentelemetry`'s `OtelGrpcLayer` for server and client |
| **Span creation** | Automatic via interceptors | Automatic via stats handlers | Automatic via `OtelGrpcLayer` middleware |
| **Log correlation** | Automatic via `LoggingInstrumentor` patching `LogRecord` | Explicit: `slog.InfoContext(ctx, ...)` — must pass context | Implicit: `tracing::info!()` inside the middleware's span automatically gets trace context |
| **OTel log bridge** | `LoggingHandler` attached to root logger by configurator | `otelslog.Handler` set as default slog handler | `OpenTelemetryTracingBridge` layer in subscriber |
| **Env var configuration** | `OTEL_*` vars read by `OpenTelemetryConfigurator` | `OTEL_*` read via `autoexport` and `autoprop` | `OTEL_SERVICE_NAME` read by SDK; `OTEL_TRACES_EXPORTER`/`OTEL_LOGS_EXPORTER` matched by `telemetry::init()` (`console`, `otlp`, `none`); `RUST_LOG` for log filtering |
| **Context passing** | Implicit via `contextvars` (thread-local) | Explicit via `context.Context` parameter | Implicit via `tracing`'s task-local span stack (managed by `OtelGrpcLayer`) |
| **Proto compilation** | `generate_protos.sh` → committed `.py` files | `generate_protos.sh` → committed `.pb.go` files | `build.rs` → generated at build time, not committed |
| **Dependency management** | `uv` + `pyproject.toml` | Go modules (`go.mod`) | Cargo (`Cargo.toml`) |

---

## Design Decisions

### 1. `tracing` over raw OTel API

Rust's `tracing` crate is the de facto structured diagnostics standard. Unlike directly using `opentelemetry::global::tracer()`, the `tracing` ecosystem provides ergonomic macros (`info!`, `info_span!`), automatic span nesting, and the subscriber/layer pattern. `tracing-opentelemetry` bridges to OTel without coupling application code to OTel's API. This means application code is backend-agnostic — you can swap OTel for any other `tracing` subscriber without changing a single log or span call.

### 2. `tonic` for gRPC

The dominant Rust gRPC framework, built on tower/hyper/prost. Tower's composable middleware (service + layer) model is analogous to Go's stats handlers. Tonic is async-first and integrates naturally with tokio.

### 3. `build.rs` for proto compilation

Rust idiom — compile protos at build time, no generated files committed. Unlike Go (committed `.pb.go`) and Python (committed `_pb2.py`), `tonic-prost-build` integrates with Cargo's build system. Tradeoff: requires `protoc` installed on the build machine.

### 4. `tonic-tracing-opentelemetry` for automatic context propagation

The `tonic-tracing-opentelemetry` crate provides tower middleware layers that are the Rust equivalent of Go's `otelgrpc.NewServerHandler()`/`NewClientHandler()`. The server `OtelGrpcLayer` extracts trace context from incoming requests and creates server spans; the client `OtelGrpcLayer` creates client spans and injects trace context into outgoing requests. This eliminates the need for manual `Injector`/`Extractor` implementations. We enable the `tracing_level_info` feature so the middleware's spans use `INFO` level (the default is `TRACE`, which would require `RUST_LOG=info,otel::tracing=trace`).

### 5. Layered subscriber vs separate concerns

The `tracing-subscriber` registry pattern composes fmt (human stderr), OTel traces, and OTel logs in one subscriber. This is Rust's "single subscriber, multiple layers" pattern. Each event flows through all layers simultaneously — a single `tracing::info!()` call produces a stderr log line, an OTel span event, and an OTel log record.

### 6. Implicit span context for logs

Unlike Go where you must call `slog.InfoContext(ctx, ...)`, in Rust `tracing::info!()` inside a handler automatically associates with the middleware's span. The `OpenTelemetryTracingBridge` picks up the trace context without explicit context passing. This is possible because `tracing` maintains a task-local span stack — the `OtelGrpcLayer` enters a span for the duration of each RPC, and any `tracing` event within inherits the span context.

### 7. Batch processor delay set to 1ms via env vars

Same reasoning as Go/Python — in development, you want spans and log records to appear immediately after each RPC, not up to 5 seconds later. The Rust SDK reads `OTEL_BSP_SCHEDULE_DELAY` (for traces) and `OTEL_BLRP_SCHEDULE_DELAY` (for logs) via `BatchConfigBuilder::init_from_env_vars()`. We set both to `1` in the run scripts, matching Python's approach. In production, use the defaults (5s for traces, 1s for logs) to amortize export overhead.

### 8. Env-var-driven exporter selection

`telemetry::init()` reads `OTEL_TRACES_EXPORTER` and `OTEL_LOGS_EXPORTER` and matches on their values (`console`, `otlp`, or `none`/unset) to select exporters at runtime. This is a simple `match` — not a full equivalent of Go's `autoexport` — but it covers the common cases and means switching exporters requires no code changes. The OTLP exporter auto-reads `OTEL_EXPORTER_OTLP_*` env vars for endpoint, protocol, and headers.

---

## Gotchas

### 1. The `TelemetryGuard` must be kept alive and dropped at the end

`TelemetryGuard` implements `Drop`, which flushes and shuts down both providers. The guard must be held in a `let` binding for the lifetime of the application — if the return value of `telemetry::init()` is discarded, the providers shut down immediately. In our code, `drop(guard)` is called explicitly at the end of `main()` to ensure buffered spans and log records are flushed before the process exits. Without this, short-lived processes (like the client) may exit before the batch processor has time to export.

### 2. `RUST_LOG` controls all output

If `RUST_LOG` is not set, the `EnvFilter` defaults to `error` only. The run scripts set `RUST_LOG=info` to ensure telemetry is visible. You can use per-module filters: `RUST_LOG=info,hyper=warn,tower=warn` to reduce noise from framework internals.

### 3. Proto compilation requires `protoc`

Unlike Go and Python where generated code is committed, Rust compiles protos at build time. If `protoc` is not installed, `cargo build` will fail with an error from `tonic-prost-build`. Install it: `brew install protobuf` (macOS) or `apt install protobuf-compiler` (Debian/Ubuntu).

### 4. The generated code needs `tonic-prost` at runtime

`tonic-prost-build` generates code that references `tonic_prost::ProstCodec`. If you forget the `tonic-prost` dependency in `[dependencies]`, you'll get an "unresolved module" error. This is easy to miss because `tonic-prost-build` (the build dependency) and `tonic-prost` (the runtime dependency) are separate crates.
