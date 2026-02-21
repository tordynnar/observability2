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
| `tonic` | The gRPC framework for Rust, built on tower/hyper. Provides `Server`, `Channel`, codegen macros, and the `MetadataMap` type used for context propagation. |
| `tonic-prost` | Runtime codec that connects tonic to prost for message serialization. The generated code references `tonic_prost::ProstCodec`. |
| `tonic-prost-build` | Build-time proto compiler. Used in `build.rs` to generate Rust types and gRPC service traits from `.proto` files. |
| `prost` | Protocol Buffers runtime for Rust. The generated message types depend on it. |
| `tokio` | Async runtime. Tonic requires tokio's multi-threaded runtime. Also provides `signal::ctrl_c()` for graceful shutdown. |

### OpenTelemetry Core

| Crate | Why |
|---|---|
| `opentelemetry` | The public API surface: `global::set_tracer_provider()`, `global::set_text_map_propagator()`, the `Injector`/`Extractor` traits for context propagation. By itself it delegates to no-op implementations until an SDK is registered. |
| `opentelemetry_sdk` | The concrete SDK: `SdkTracerProvider`, `BatchSpanProcessor`, `SdkLoggerProvider`. This is what actually records spans and log records and exports them. |
| `opentelemetry-stdout` | Exports spans and log records as structured output to stdout. The development-time equivalent of an OTLP exporter. |

### Bridges: `tracing` ↔ OpenTelemetry

| Crate | Why |
|---|---|
| `tracing` | Rust's de facto structured diagnostics crate. Provides `info!`, `info_span!`, the `Instrument` trait, and the subscriber/layer pattern. All application logging and span creation goes through `tracing`. |
| `tracing-subscriber` | Composable subscriber layers. We use `fmt::Layer` (human-readable stderr), `EnvFilter` (controls log levels via `RUST_LOG`), and the `Registry` that holds all layers. |
| `tracing-opentelemetry` | Bridges `tracing` spans → OTel spans. The `OpenTelemetryLayer` converts every `tracing::Span` into an OTel span, exported via the `SdkTracerProvider`. Also provides `OpenTelemetrySpanExt` for `set_parent()` and `.context()`. |
| `opentelemetry-appender-tracing` | Bridges `tracing` events → OTel log records. The `OpenTelemetryTracingBridge` converts every `tracing::info!()` call into an OTel `LogRecord`, exported via the `SdkLoggerProvider`. |

---

## How It Works

### Explicit Initialization (No Auto-Instrumentation)

Like Go, Rust has no `initialize()` magic call or monkey-patching. Every piece of instrumentation is wired explicitly in code:

```rust
// src/telemetry.rs — called once at startup
let guard = telemetry::init();

// src/bin/server.rs — manual span creation and context extraction per RPC
let parent_cx = telemetry::extract_trace_context(request.metadata());
let span = tracing::info_span!("helloworld.Greeter/SayHello", otel.kind = "server", ...);
span.set_parent(parent_cx);

// src/bin/client.rs — manual span creation and context injection per RPC
let span = tracing::info_span!("helloworld.Greeter/SayHello", otel.kind = "client", ...);
telemetry::inject_trace_context(request.metadata_mut());
```

`telemetry::init()` does the following:

1. **Creates a `Resource`** by reading `OTEL_SERVICE_NAME` from the environment.
2. **Creates a `SdkTracerProvider`** with a `BatchSpanProcessor` wrapping a stdout `SpanExporter`.
3. **Creates a `SdkLoggerProvider`** with a batch exporter wrapping a stdout `LogExporter`.
4. **Sets the global propagator** to W3C `TraceContextPropagator`.
5. **Builds a layered `tracing` subscriber** with three layers (see below).
6. **Returns a `TelemetryGuard`** whose `shutdown()` method flushes both providers.

### The `tracing` Crate: Rust's Unified Diagnostics Layer

Unlike Go (which has separate `log/slog` and `otel` APIs) or Python (which has separate `logging` and `opentelemetry` packages), Rust's `tracing` crate serves as the single entry point for both structured logging and span creation. Application code only calls `tracing::info!()` and `tracing::info_span!()` — it never touches the OpenTelemetry API directly.

The `tracing` ecosystem uses a **subscriber pattern**: you register a global subscriber composed of multiple layers, and each layer independently processes every span and event. Our subscriber has three layers:

1. **`fmt::Layer`** — writes human-readable log lines to stderr. This is what you see during development.
2. **`OpenTelemetryLayer`** (from `tracing-opentelemetry`) — converts every `tracing::Span` into an OTel span, which gets exported as structured output to stdout via the `SdkTracerProvider`.
3. **`OpenTelemetryTracingBridge`** (from `opentelemetry-appender-tracing`) — converts every `tracing::info!()` event into an OTel `LogRecord`, which gets exported to stdout via the `SdkLoggerProvider`.

### Implicit Span Context for Logs

Unlike Go where you must call `slog.InfoContext(ctx, ...)` to correlate logs with traces, in Rust `tracing::info!()` inside an `.instrument(span)` block **automatically** associates with the parent span. The `OpenTelemetryTracingBridge` picks up the trace context without explicit context passing:

```rust
// Go — context passing is explicit:
slog.InfoContext(ctx, "Received request", "name", req.GetName())

// Rust — context is implicit via the instrumented span:
async {
    tracing::info!(name = %name, "Received request");
    // ^^^ automatically gets trace_id/span_id from the enclosing span
}.instrument(span).await
```

This is because `tracing` uses a thread-local (or task-local in async) span stack. When you call `.instrument(span)`, the span is entered for the duration of the async block, and any `tracing::info!()` call within automatically inherits the span context.

### Context Propagation via MetadataInjector/MetadataExtractor

Since there's no equivalent to Go's `otelgrpc.NewServerHandler()` that auto-creates spans and propagates context, we implement the `Injector` and `Extractor` traits for tonic's `MetadataMap` manually:

- **`MetadataInjector`** — implements `opentelemetry::propagation::Injector`, allowing the propagator to write `traceparent` into outgoing gRPC metadata.
- **`MetadataExtractor`** — implements `opentelemetry::propagation::Extractor`, allowing the propagator to read `traceparent` from incoming gRPC metadata.

Two helper functions wrap the propagator calls:
- `inject_trace_context(metadata)` — gets the current span's OTel context via `tracing_opentelemetry::OpenTelemetrySpanExt::context()` and injects it.
- `extract_trace_context(metadata)` — extracts and returns an `opentelemetry::Context` that can be set as a span's parent.

---

## Environment Variables

The launch scripts set these environment variables.

| Variable | Value | Read by |
|---|---|---|
| `OTEL_SERVICE_NAME` | `grpc-server` / `grpc-client` | `telemetry::init()` — sets `service.name` on the resource attached to all spans and log records. |
| `OTEL_TRACES_EXPORTER` | `console` | Not read by code (stdout exporter is hardcoded). Set for documentation consistency with Go/Python. |
| `OTEL_LOGS_EXPORTER` | `console` | Not read by code (stdout exporter is hardcoded). Set for documentation consistency with Go/Python. |
| `OTEL_PROPAGATORS` | `tracecontext,baggage` | Not read by code (W3C TraceContext is hardcoded). Set for documentation consistency. |
| `OTEL_BSP_SCHEDULE_DELAY` | `1` | `BatchSpanProcessor` schedule delay in milliseconds (default: 5000). Read by the SDK's `BatchConfigBuilder` via `init_from_env_vars()`. Setting to `1` means spans appear within ~1ms of completion. |
| `OTEL_BLRP_SCHEDULE_DELAY` | `1` | `BatchLogProcessor` schedule delay in milliseconds (default: 1000). Read by the SDK's `BatchConfigBuilder` via `init_from_env_vars()`. Setting to `1` means log records appear immediately. |
| `RUST_LOG` | `info` | `tracing_subscriber::EnvFilter` — controls which `tracing` events are emitted. Supports per-module filtering (e.g., `RUST_LOG=info,hyper=warn`). |

---

## Exporting via OTLP Instead of Console

The stdout exporter is useful for seeing raw telemetry during development, but in a real setup you'll send telemetry to a collector or backend via OTLP. Unlike Go's `autoexport` which reads env vars to select exporters at runtime, Rust requires a code change.

### What to Change

1. Add the OTLP exporter crate:
   ```toml
   opentelemetry-otlp = { version = "0.31", features = ["grpc-tonic"] }
   ```

2. In `telemetry.rs`, replace the stdout exporters:
   ```rust
   // Before:
   let span_exporter = opentelemetry_stdout::SpanExporter::default();
   let log_exporter = opentelemetry_stdout::LogExporter::default();

   // After:
   use opentelemetry_otlp::SpanExporter;
   use opentelemetry_otlp::LogExporter;
   let span_exporter = SpanExporter::builder().with_tonic().build()?;
   let log_exporter = LogExporter::builder().with_tonic().build()?;
   ```

3. Set the endpoint in your environment:
   ```bash
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

### 1. Client creates a span

The client code creates a `tracing::info_span!` with `otel.kind = "client"`. The `OpenTelemetryLayer` converts this into an OTel span with `SpanKind=CLIENT`. Inside the instrumented async block, `inject_trace_context()` calls the global propagator's `inject()`, which writes a `traceparent` header into the gRPC metadata:
```
traceparent: 00-8267a68f2f0b720634bb2d30d0710acb-5d2a1bf665bbc02b-01
```

### 2. Server extracts the context and creates a child span

The server's `say_hello` handler calls `extract_trace_context(request.metadata())`, which parses the `traceparent` header and reconstructs the remote `SpanContext`. It then creates an `info_span!` with `otel.kind = "server"` and calls `span.set_parent(parent_cx)` to link it. The server span gets the **same TraceId** as the client span, and its ParentSpanId is the client span's SpanId.

### 3. Log correlation happens automatically

Inside the `.instrument(span)` block, `tracing::info!()` events automatically carry the span's trace context. The `OpenTelemetryTracingBridge` converts these into OTel `LogRecord`s with `trace_id` and `span_id` fields.

### 4. Verification: matching IDs

```
# Client span (stdout):
TraceId      : 8267a68f2f0b720634bb2d30d0710acb
SpanId       : 5d2a1bf665bbc02b
ParentSpanId : None (root span)
Kind         : Client

# Server span (stdout):
TraceId      : 8267a68f2f0b720634bb2d30d0710acb    # same trace!
SpanId       : f7a0c0538352f217
ParentSpanId : 5d2a1bf665bbc02b                      # matches client SpanId
Kind         : Server

# Server OTel log record (stdout):
TraceId: 8267a68f2f0b720634bb2d30d0710acb            # same trace as both spans
SpanId: f7a0c0538352f217                              # matches server span
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

Controlled by: stdout exporter hardcoded in `telemetry.rs`

---

## Rust vs Go vs Python

| Aspect | Python | Go | Rust |
|---|---|---|---|
| **Initialization** | `initialize()` — discovers and activates instrumentors + SDK via entry points and env vars | Explicit: create providers, set globals, pass stats handlers | Explicit: create providers, build layered `tracing` subscriber |
| **gRPC instrumentation** | Monkey-patching via `GrpcInstrumentorServer/Client` | Stats handlers: `otelgrpc.NewServerHandler()`/`NewClientHandler()` | Manual: `Injector`/`Extractor` impls for `MetadataMap`, explicit span creation per RPC |
| **Span creation** | Automatic via interceptors | Automatic via stats handlers | Manual: `tracing::info_span!()` + `.instrument()` |
| **Log correlation** | Automatic via `LoggingInstrumentor` patching `LogRecord` | Explicit: `slog.InfoContext(ctx, ...)` — must pass context | Implicit: `tracing::info!()` inside `.instrument(span)` automatically gets trace context |
| **OTel log bridge** | `LoggingHandler` attached to root logger by configurator | `otelslog.Handler` set as default slog handler | `OpenTelemetryTracingBridge` layer in subscriber |
| **Env var configuration** | `OTEL_*` vars read by `OpenTelemetryConfigurator` | `OTEL_*` read via `autoexport` and `autoprop` | `OTEL_SERVICE_NAME` read by code; `RUST_LOG` for log filtering; exporters hardcoded |
| **Context passing** | Implicit via `contextvars` (thread-local) | Explicit via `context.Context` parameter | Implicit via `tracing`'s task-local span stack (within `.instrument()`) |
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

### 4. Manual context propagation

There's no Rust equivalent to Go's `otelgrpc.NewServerHandler()` that auto-creates spans and propagates context for every RPC. We implement `Injector`/`Extractor` for tonic's `MetadataMap` — explicit but clear. This is two small struct impls plus two helper functions in `telemetry.rs`.

### 5. Layered subscriber vs separate concerns

The `tracing-subscriber` registry pattern composes fmt (human stderr), OTel traces, and OTel logs in one subscriber. This is Rust's "single subscriber, multiple layers" pattern. Each event flows through all layers simultaneously — a single `tracing::info!()` call produces a stderr log line, an OTel span event, and an OTel log record.

### 6. Implicit span context for logs

Unlike Go where you must call `slog.InfoContext(ctx, ...)`, in Rust `tracing::info!()` inside an `.instrument(span)` block automatically associates with the parent span. The `OpenTelemetryTracingBridge` picks up the trace context without explicit context passing. This is possible because `tracing` maintains a task-local span stack — when a span is "entered" (via `.instrument()`), it becomes the current span for the duration of that async block.

### 7. Batch processor delay set to 1ms via env vars

Same reasoning as Go/Python — in development, you want spans and log records to appear immediately after each RPC, not up to 5 seconds later. The Rust SDK reads `OTEL_BSP_SCHEDULE_DELAY` (for traces) and `OTEL_BLRP_SCHEDULE_DELAY` (for logs) via `BatchConfigBuilder::init_from_env_vars()`. We set both to `1` in the run scripts, matching Python's approach. In production, use the defaults (5s for traces, 1s for logs) to amortize export overhead.

### 8. No env-var-driven exporter selection

Unlike Go's `autoexport` package which reads `OTEL_TRACES_EXPORTER` to select between console/OTLP/none at runtime, Rust's OTel ecosystem has no equivalent. The exporter is selected at compile time. We hardcode the stdout exporter; see [Exporting via OTLP](#exporting-via-otlp-instead-of-console) for how to swap.

---

## Gotchas

### 1. `otel.kind` must be a string, not a SpanKind enum

When setting `otel.kind` in `info_span!`, use the string `"server"` or `"client"`. The `tracing-opentelemetry` layer recognizes these string values and maps them to the corresponding `SpanKind` enum. If you omit `otel.kind`, the span defaults to `SpanKind::Internal`.

### 2. `set_parent()` returns a `Result`

`span.set_parent(context)` returns a `Result` in `tracing-opentelemetry` 0.32. If the span is closed or the context is invalid, it fails silently. Use `let _ = span.set_parent(...)` to acknowledge the result.

### 3. Shutdown must be called

If the process exits without calling `guard.shutdown()`, buffered spans and log records may be lost. The `BatchSpanProcessor` flushes on shutdown. In our code, `guard.shutdown()` is called at the end of `main()`.

### 4. `RUST_LOG` controls all output

If `RUST_LOG` is not set, the `EnvFilter` defaults to `error` only. The run scripts set `RUST_LOG=info` to ensure telemetry is visible. You can use per-module filters: `RUST_LOG=info,hyper=warn,tower=warn` to reduce noise from framework internals.

### 5. Proto compilation requires `protoc`

Unlike Go and Python where generated code is committed, Rust compiles protos at build time. If `protoc` is not installed, `cargo build` will fail with an error from `tonic-prost-build`. Install it: `brew install protobuf` (macOS) or `apt install protobuf-compiler` (Debian/Ubuntu).

### 6. The generated code needs `tonic-prost` at runtime

`tonic-prost-build` generates code that references `tonic_prost::ProstCodec`. If you forget the `tonic-prost` dependency in `[dependencies]`, you'll get an "unresolved module" error. This is easy to miss because `tonic-prost-build` (the build dependency) and `tonic-prost` (the runtime dependency) are separate crates.
