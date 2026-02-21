# gRPC + OpenTelemetry in Go: A Complete Guide

This project demonstrates distributed tracing and log/trace correlation for a Go gRPC application using OpenTelemetry. All telemetry is exported to the console so you can see exactly what the SDK produces.

## What This Project Does

A gRPC `Greeter` service with a single `SayHello` RPC. A client sends a request; the server responds. OpenTelemetry instruments both sides, producing:

1. **Distributed traces** — a CLIENT span on the caller, a SERVER span on the callee, linked by W3C Trace Context propagated through gRPC metadata.
2. **OTel log records** — `slog` records bridged into the OpenTelemetry Logs pipeline and exported as structured JSON to stdout, with trace_id/span_id for correlation.

## Project Structure

```
go/
├── protos/
│   └── helloworld.proto              # gRPC service definition (with go_package option)
├── pb/
│   ├── helloworld.pb.go              # Generated protobuf code (committed)
│   └── helloworld_grpc.pb.go         # Generated gRPC code (committed)
├── cmd/
│   ├── server/
│   │   └── main.go                   # gRPC server with OTel stats handler
│   └── client/
│       └── main.go                   # gRPC client with OTel stats handler
├── internal/
│   └── telemetry/
│       └── telemetry.go              # OTel initialization (providers, exporters, slog setup)
├── generate_protos.sh                # Proto compilation script
├── run_server.sh                     # Launch script with OTEL_* env vars
├── run_client.sh                     # Launch script with OTEL_* env vars
├── go.mod
├── go.sum
└── README.md
```

## Quick Start

```bash
cd go

# Terminal 1: start the server
chmod +x run_server.sh run_client.sh
./run_server.sh

# Terminal 2: send a request
./run_client.sh
```

No proto generation step needed — the generated `.pb.go` files are committed. To regenerate after changing the proto:

```bash
chmod +x generate_protos.sh
./generate_protos.sh
```

This requires `protoc`, `protoc-gen-go`, and `protoc-gen-go-grpc`:

```bash
go install google.golang.org/protobuf/cmd/protoc-gen-go@latest
go install google.golang.org/grpc/cmd/protoc-gen-go-grpc@latest
```

---

## Dependencies

Requires **Go 1.21+** (for `log/slog`). Dependencies are managed by Go modules (`go.mod`).

### gRPC

| Package | Why |
|---|---|
| `google.golang.org/grpc` | The gRPC runtime. Provides `grpc.NewServer()`, `grpc.NewClient()`, stats handlers, and transport. |
| `google.golang.org/protobuf` | Protocol Buffers runtime for Go. The generated `.pb.go` files depend on it. |

### OpenTelemetry Core

| Package | Why |
|---|---|
| `go.opentelemetry.io/otel` | The public API surface: `otel.SetTracerProvider()`, `otel.SetTextMapPropagator()`. By itself it delegates to no-op implementations until an SDK is registered. |
| `go.opentelemetry.io/otel/sdk` | The concrete SDK: `TracerProvider`, `BatchSpanProcessor`. This is what actually records spans and exports them. |
| `go.opentelemetry.io/otel/sdk/log` | The Logs SDK: `LoggerProvider`, `BatchProcessor`. Records OTel log records and exports them. |

### Env-Var-Driven Configuration

| Package | Why |
|---|---|
| `go.opentelemetry.io/contrib/exporters/autoexport` | Reads `OTEL_TRACES_EXPORTER` and `OTEL_LOGS_EXPORTER` env vars and returns the matching exporter. Supports `"console"` (stdout JSON), `"otlp"` (send to a collector), and `"none"` (no-op). |
| `go.opentelemetry.io/contrib/propagators/autoprop` | Reads `OTEL_PROPAGATORS` env var and returns the matching composite propagator. Defaults to `tracecontext,baggage`. Also supports `b3`, `b3multi`, `jaeger`, `xray`, `ottrace`, and `none`. |

### Instrumentation

| Package | Why |
|---|---|
| `go.opentelemetry.io/contrib/instrumentation/google.golang.org/grpc/otelgrpc` | Provides `NewServerHandler()` and `NewClientHandler()` — gRPC stats handlers that automatically create spans for every RPC. The server handler extracts trace context from incoming metadata; the client handler injects it into outgoing metadata. |
| `go.opentelemetry.io/contrib/bridges/otelslog` | Bridges `log/slog` into the OTel Logs pipeline. Each `slog` record becomes an OTel `LogRecord` with trace context, processed by `BatchProcessor` and exported. |

---

## How It Works

### Explicit Initialization (No Auto-Instrumentation)

Unlike Python, Go has no `initialize()` magic call or monkey-patching. Every piece of instrumentation is wired explicitly in code:

```go
// internal/telemetry/telemetry.go — called once at startup
shutdown, err := telemetry.Init(ctx)

// cmd/server/main.go — stats handler passed to gRPC server
srv := grpc.NewServer(grpc.StatsHandler(otelgrpc.NewServerHandler()))

// cmd/client/main.go — stats handler passed to gRPC client
conn, err := grpc.NewClient("localhost:50051",
    grpc.WithStatsHandler(otelgrpc.NewClientHandler()),
)
```

`telemetry.Init()` does the following:

1. **Creates a `resource.Resource`** via `resource.WithFromEnv()`, which reads `OTEL_SERVICE_NAME` and `OTEL_RESOURCE_ATTRIBUTES` from the environment.
2. **Creates a `TracerProvider`** with the exporter selected by `OTEL_TRACES_EXPORTER` (via `autoexport`) and a `BatchSpanProcessor`.
3. **Creates a `LoggerProvider`** with the exporter selected by `OTEL_LOGS_EXPORTER` (via `autoexport`) and a `BatchProcessor`.
4. **Sets the global propagator** via `autoprop.NewTextMapPropagator()`, which reads `OTEL_PROPAGATORS`.
5. **Bridges `slog` to the OTel Logs pipeline** — sets `otelslog.Handler` as the default slog handler.
6. **Returns a shutdown function** that flushes and stops both providers.

### Why Stats Handlers, Not Interceptors

The `otelgrpc` package offers both interceptors and stats handlers. We use stats handlers because:

- The interceptors (`UnaryServerInterceptor`, `UnaryClientInterceptor`) are **deprecated** in favor of stats handlers.
- Stats handlers cover both unary and streaming RPCs with a single registration point.
- Stats handlers are the recommended approach in the [otelgrpc documentation](https://pkg.go.dev/go.opentelemetry.io/contrib/instrumentation/google.golang.org/grpc/otelgrpc).

### Context Passing Is Explicit

In Go, there is no thread-local context. To correlate logs with traces, every log call must explicitly pass the `context.Context` that carries the span:

```go
slog.InfoContext(ctx, "Received request", "name", req.GetName())
//               ^^^ this is what connects the log to the trace
```

If you use `slog.Info()` (without context), the log record will have zero trace_id/span_id — there's no way for the handler to find the active span.

---

## Environment Variables

The launch scripts set these environment variables. All are read by the Go code via `resource.WithFromEnv()`, `autoexport`, and `autoprop`.

| Variable | Value | Read by |
|---|---|---|
| `OTEL_SERVICE_NAME` | `grpc-server` / `grpc-client` | `resource.WithFromEnv()` — sets `service.name` on the resource attached to all spans and log records. |
| `OTEL_TRACES_EXPORTER` | `console` | `autoexport.NewSpanExporter()` — `"console"` exports spans as JSON to stdout. Also supports `"otlp"` and `"none"`. |
| `OTEL_LOGS_EXPORTER` | `console` | `autoexport.NewLogExporter()` — `"console"` exports log records as JSON to stdout. Also supports `"otlp"` and `"none"`. |
| `OTEL_PROPAGATORS` | `tracecontext,baggage` | `autoprop.NewTextMapPropagator()` — selects W3C Trace Context + Baggage propagators. Also supports `b3`, `jaeger`, `xray`, etc. |

---

## Exporting via OTLP Instead of Console

The `console` exporter is useful for seeing raw telemetry during development, but in a real setup you'll send telemetry to a collector or observability backend via OTLP (OpenTelemetry Protocol). The `autoexport` package already includes OTLP exporter support (both gRPC and HTTP variants are in `go.mod` as indirect dependencies), so no additional packages are needed — just change the environment variables.

### Minimal Change

In your launch scripts (`run_server.sh`, `run_client.sh`), replace:

```bash
export OTEL_TRACES_EXPORTER="console"
export OTEL_LOGS_EXPORTER="console"
```

with:

```bash
export OTEL_TRACES_EXPORTER="otlp"
export OTEL_LOGS_EXPORTER="otlp"
```

By default, the OTLP exporter sends telemetry to `http://localhost:4317` (gRPC) or `http://localhost:4318` (HTTP). If you have an OpenTelemetry Collector or a compatible backend (Jaeger, Grafana Tempo, etc.) listening there, spans and log records will start flowing immediately.

### OTLP Configuration Variables

| Variable | Default | What It Does |
|---|---|---|
| `OTEL_EXPORTER_OTLP_ENDPOINT` | `http://localhost:4317` (gRPC) or `http://localhost:4318` (HTTP) | The collector endpoint. Set this to point at your collector or backend. |
| `OTEL_EXPORTER_OTLP_PROTOCOL` | `http/protobuf` | Protocol to use. `grpc` sends to port 4317; `http/protobuf` sends to port 4318. |
| `OTEL_EXPORTER_OTLP_HEADERS` | *(none)* | Comma-separated `key=value` pairs sent as headers on every export request. Used for authentication tokens (e.g., `x-api-key=secret`). |
| `OTEL_EXPORTER_OTLP_CERTIFICATE` | *(none)* | Path to a TLS certificate file for the collector connection. |

You can also set signal-specific overrides like `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` or `OTEL_EXPORTER_OTLP_LOGS_ENDPOINT` if traces and logs go to different destinations.

### Example: Sending to a Local Collector

```bash
export OTEL_SERVICE_NAME="grpc-server"
export OTEL_TRACES_EXPORTER="otlp"
export OTEL_LOGS_EXPORTER="otlp"
export OTEL_EXPORTER_OTLP_ENDPOINT="http://localhost:4317"
export OTEL_EXPORTER_OTLP_PROTOCOL="grpc"
export OTEL_PROPAGATORS="tracecontext,baggage"
```

No code changes are needed — `autoexport.NewSpanExporter()` and `autoexport.NewLogExporter()` in `telemetry.go` already read `OTEL_TRACES_EXPORTER` and `OTEL_LOGS_EXPORTER` to select the exporter at runtime.

---

## How Distributed Tracing Works Across gRPC

Here's the exact flow when the client calls `SayHello`:

### 1. Client creates a span

The `otelgrpc.NewClientHandler()` stats handler intercepts the outgoing RPC. It:

- Creates a new span with `SpanKind=CLIENT` and name `helloworld.Greeter/SayHello`
- Sets attributes: `rpc.system.name=grpc`, `rpc.method=helloworld.Greeter/SayHello`
- Calls `inject()` on the global propagator, which writes a `traceparent` header into the gRPC metadata:
  ```
  traceparent: 00-5061a996070a831e1a8ef895f8aa4268-d0fe6574fa40d555-01
  ```

### 2. Server extracts the context and creates a child span

The `otelgrpc.NewServerHandler()` stats handler intercepts the incoming RPC. It:

- Calls `extract()` on the global propagator, which parses the `traceparent` header and reconstructs the remote `SpanContext`
- Creates a new span with `SpanKind=SERVER`, setting the extracted context as the parent
- The server span gets the **same `TraceID`** as the client span, and its `Parent.SpanID` is the client span's `SpanID`

### 3. Log correlation happens via explicit context passing

Inside the `SayHello` handler, the server span is in the context. When `slog.InfoContext(ctx, ...)` is called, the `otelslog.Handler` bridges the record into an OTel `LogRecord` with `TraceID` and `SpanID` fields, which gets exported as JSON to stdout.

### 4. Verification: matching IDs

```
# Client span (stdout):
"TraceID": "5061a996070a831e1a8ef895f8aa4268"
"SpanID":  "d0fe6574fa40d555"
SpanKind:  3 (CLIENT)
Parent:    zero (root span)

# Server span (stdout):
"TraceID": "5061a996070a831e1a8ef895f8aa4268"     # same trace!
"SpanID":  "fc7fba7097ddcd66"
SpanKind:  2 (SERVER)
Parent:    "d0fe6574fa40d555"                       # matches client SpanID

# Server OTel log record (stdout JSON):
"TraceID": "5061a996070a831e1a8ef895f8aa4268"       # same trace as both spans
"SpanID":  "fc7fba7097ddcd66"                        # matches server span
```

---

## Two Kinds of Telemetry Output

### 1. OTel spans to stdout (JSON)

```json
{
    "Name": "helloworld.Greeter/SayHello",
    "SpanContext": {
        "TraceID": "5061a996070a831e1a8ef895f8aa4268",
        "SpanID": "fc7fba7097ddcd66"
    },
    "SpanKind": 2,
    "Parent": {
        "TraceID": "5061a996070a831e1a8ef895f8aa4268",
        "SpanID": "d0fe6574fa40d555"
    },
    "Attributes": [
        {"Key": "rpc.system.name", "Value": {"Type": "STRING", "Value": "grpc"}},
        {"Key": "rpc.method", "Value": {"Type": "STRING", "Value": "helloworld.Greeter/SayHello"}}
    ],
    "Resource": [
        {"Key": "service.name", "Value": {"Type": "STRING", "Value": "grpc-server"}}
    ]
}
```

Produced by: `autoexport` console exporter via `BatchSpanProcessor`

### 2. OTel log records to stdout (JSON)

```json
{
    "Severity": 9,
    "SeverityText": "INFO",
    "Body": {"Type": "String", "Value": "Received request"},
    "Attributes": [{"Key": "name", "Value": {"Type": "String", "Value": "World"}}],
    "TraceID": "5061a996070a831e1a8ef895f8aa4268",
    "SpanID": "fc7fba7097ddcd66",
    "Resource": [{"Key": "service.name", "Value": {"Type": "STRING", "Value": "grpc-server"}}]
}
```

Produced by: `otelslog.Handler` → `autoexport` console exporter via `BatchProcessor`

---

## Go vs Python Differences

| Aspect | Python | Go |
|---|---|---|
| **Initialization** | `initialize()` — discovers and activates instrumentors + SDK via entry points and env vars | Explicit: create providers, set globals, pass stats handlers to gRPC constructors |
| **gRPC instrumentation** | Monkey-patching via `GrpcInstrumentorServer/Client` | Stats handlers: `otelgrpc.NewServerHandler()`/`NewClientHandler()` passed to `grpc.NewServer()`/`grpc.NewClient()` |
| **Log correlation** | Automatic via `LoggingInstrumentor` patching `LogRecord` | Explicit: `slog.InfoContext(ctx, ...)` — must pass context manually |
| **OTel log bridge** | `LoggingHandler` attached to root logger by configurator | `otelslog.Handler` set as default slog handler |
| **Env var configuration** | `OTEL_*` vars read by `OpenTelemetryConfigurator` to build entire SDK | `OTEL_SERVICE_NAME`, `OTEL_TRACES_EXPORTER`, `OTEL_LOGS_EXPORTER`, `OTEL_PROPAGATORS` read via `resource.WithFromEnv()`, `autoexport`, and `autoprop` |
| **Import order** | Critical: `initialize()` must come before `import grpc` | Not applicable: no monkey-patching |
| **Thread-local context** | Yes (`contextvars`): active span is implicitly available | No: context must be explicitly passed through `context.Context` |
| **Proto file** | No `go_package` option needed | Requires `option go_package` (proto file copied, not symlinked) |
| **Generated code** | `helloworld_pb2.py`, `helloworld_pb2_grpc.py` | `pb/helloworld.pb.go`, `pb/helloworld_grpc.pb.go` |
| **Dependency management** | `uv` + `pyproject.toml` | Go modules (`go.mod`) |

---

## Design Decisions

### 1. Proto file copied, not symlinked

The Go protoc plugins require a `go_package` option in the `.proto` file that the Python proto doesn't have. Rather than complicate the Python proto with Go-specific options or use symlinks (which create cross-platform issues), we copy the proto and add the option.

### 2. Generated proto files committed

Go convention is to commit generated code. This matches the Python project (which also commits generated files) and simplifies quick start — you don't need protoc installed to build and run.

### 3. `log/slog` for logging

Go's stdlib structured logger (available since Go 1.21). It supports `context.Context` natively via `InfoContext`/`WarnContext`/etc., which is essential for trace correlation. The `otelslog` bridge package connects it directly to the OTel Logs pipeline.

### 4. Batch processor timeout set to 1ms

Same reasoning as the Python project's `OTEL_BSP_SCHEDULE_DELAY=1`: in development, you want spans and log records to appear immediately after each RPC, not up to 5 seconds later. The Go SDK does not read `OTEL_BSP_SCHEDULE_DELAY`, so this is set programmatically. In production, use the default (5s) to amortize export overhead.

---

## Gotchas

### 1. `slog.Info()` vs `slog.InfoContext(ctx, ...)`

If you forget to pass `ctx`, the log record has zero trace_id/span_id. There's no compiler warning — `slog.Info()` is a perfectly valid call, it just doesn't correlate with traces. Always use the `*Context` variants.

### 2. Stats handlers vs interceptors

The `otelgrpc` package still exports `UnaryServerInterceptor` and `UnaryClientInterceptor`, but they are deprecated. Use `NewServerHandler()`/`NewClientHandler()` with `grpc.StatsHandler()`.

### 3. Shutdown must be called

If the process exits without calling the shutdown function returned by `telemetry.Init()`, buffered spans and log records may be lost. The `BatchSpanProcessor` and `BatchProcessor` flush on shutdown. In our code, `defer shutdown(ctx)` in `main()` handles this.

### 4. No auto-discovery of instrumentors

Unlike Python's `initialize()` which scans entry points to find all installed instrumentors, Go requires you to explicitly pass stats handlers (or interceptors) to each gRPC server/client. If you add a new gRPC service and forget to add the stats handler, that service produces no spans.
