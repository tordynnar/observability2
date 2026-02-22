---
name: otel-grpc
description: "How to add OpenTelemetry distributed tracing and correlated logs to gRPC services in Python, Go, and Rust. Use this skill whenever the user wants to add observability, tracing, telemetry, or log correlation to a gRPC application. Also use when the user mentions OpenTelemetry, OTel, distributed tracing, span propagation, or trace-correlated logging in the context of gRPC services -- even if they don't explicitly say 'OpenTelemetry'. Trigger on language-specific contexts too: Python gRPC tracing, Go gRPC observability, Rust tonic tracing, grpcio instrumentation, otelgrpc, tonic-tracing-opentelemetry, or any combination of these languages with gRPC and telemetry concepts. Covers all three languages with complete code examples, dependency lists, environment variable configuration, and troubleshooting."
---

# OpenTelemetry + gRPC: Distributed Tracing & Correlated Logs

This skill covers adding OpenTelemetry to gRPC services in **Python**, **Go**, and **Rust**. The approach produces two telemetry signals:

1. **Distributed traces** -- CLIENT spans on callers, SERVER spans on callees, linked by W3C Trace Context propagated through gRPC metadata.
2. **Correlated log records** -- application logs bridged into the OTel Logs pipeline with trace_id/span_id for correlation.

Metrics are deliberately excluded to keep the setup focused. Add them later if needed.

## Architecture Overview

All three languages follow the same conceptual flow:

```
1. Initialize OTel SDK (TracerProvider + LoggerProvider)
2. Set W3C TraceContext as the global propagator
3. Attach gRPC instrumentation (auto spans + context propagation)
4. Bridge application logging into the OTel Logs pipeline
5. Configure via OTEL_* environment variables (no code changes)
6. Shut down providers on exit to flush buffered telemetry
```

The implementation differs by language, but the concepts are identical. Each language has a dedicated reference file with complete code and dependency lists.

## Language-Specific References

Read the reference file for the language you're working in:

| Language | Reference | Key Pattern |
|----------|-----------|-------------|
| **Python** | `references/python.md` | `initialize()` + import ordering -- fully automatic, zero OTel API calls in app code |
| **Go** | `references/go.md` | Explicit `telemetry.Init()` + stats handlers -- `slog.InfoContext(ctx)` for log correlation |
| **Rust** | `references/rust.md` | `telemetry::init()` + `tracing` subscriber layers -- implicit span context via task-local stack |

Each reference includes: dependencies, initialization code, server/client examples, environment variables, design decisions (with rationale), how to test, and troubleshooting.

## Cross-Language Comparison

| Aspect | Python | Go | Rust |
|--------|--------|-----|------|
| **Initialization** | `initialize()` -- discovers instrumentors + SDK via entry points | Explicit: create providers, set globals, pass stats handlers | Explicit: create providers, build layered `tracing` subscriber |
| **gRPC instrumentation** | Monkey-patching via auto-instrumentors | Stats handlers: `NewServerHandler()` / `NewClientHandler()` | Tower middleware: `OtelGrpcLayer` |
| **Log correlation** | Automatic via `LoggingInstrumentor` | Explicit: must use `slog.InfoContext(ctx, ...)` | Implicit: `tracing::info!()` inherits context |
| **OTel log bridge** | `LoggingHandler` on root logger | `otelslog.Handler` as default slog handler | `OpenTelemetryTracingBridge` subscriber layer |
| **Context passing** | Implicit (`contextvars`) | Explicit (`context.Context` parameter) | Implicit (task-local span stack) |
| **Proto compilation** | `generate_protos.sh` -> committed `.py` files | `generate_protos.sh` -> committed `.pb.go` files | `build.rs` -> generated at build time, not committed |

## Common Environment Variables

These env vars work identically across all three languages. Set them in shell scripts to configure telemetry without code changes.

### Core

| Variable | Dev Value | Purpose |
|----------|-----------|---------|
| `OTEL_SERVICE_NAME` | `grpc-server` / `grpc-client` | Identifies the service in all telemetry |
| `OTEL_TRACES_EXPORTER` | `none` | Exporter for spans: `none` (disabled), `console` (stdout), `otlp` (collector) |
| `OTEL_LOGS_EXPORTER` | `none` | Exporter for log records: `none`, `console`, `otlp` |
| `OTEL_PROPAGATORS` | `tracecontext,baggage` | W3C Trace Context propagation |

### Batch Processor Tuning

| Variable | Dev Value | Default | Purpose |
|----------|-----------|---------|---------|
| `OTEL_BSP_SCHEDULE_DELAY` | `1` | `5000` | Span batch flush interval (ms). Set to `1` for near-instant dev output. |
| `OTEL_BLRP_SCHEDULE_DELAY` | `1` | `1000`-`5000` | Log record batch flush interval (ms). Set to `1` for near-instant dev output. |

**Why 1ms in dev:** Batch processors buffer telemetry and flush periodically. With defaults (5 seconds for spans), you'd wait up to 5 seconds after a request before seeing output. Setting to 1ms gives near-instant feedback. Use defaults in production to amortize export overhead.

### Switching Exporters

No code changes needed -- just change the environment variables. Use `none` in development, `console` to confirm tracing/logging is working, and `otlp` in production:

```bash
# Development: telemetry disabled (no output noise)
export OTEL_TRACES_EXPORTER="none"
export OTEL_LOGS_EXPORTER="none"

# Verification: confirm tracing and log correlation are working
export OTEL_TRACES_EXPORTER="console"
export OTEL_LOGS_EXPORTER="console"

# Production: export to a collector
export OTEL_TRACES_EXPORTER="otlp"
export OTEL_LOGS_EXPORTER="otlp"
export OTEL_EXPORTER_OTLP_ENDPOINT="http://localhost:4317"
# Optional:
# export OTEL_EXPORTER_OTLP_PROTOCOL="grpc"
# export OTEL_EXPORTER_OTLP_HEADERS="x-api-key=secret"
```

## How to Verify It Works

The verification process is the same across all languages. First, switch to the `console` exporter (`OTEL_TRACES_EXPORTER=console`, `OTEL_LOGS_EXPORTER=console`) so telemetry is printed to stdout. Then start the server and run the client:

### 1. Check for matching trace IDs

The client span and server span must share the same `trace_id`. The server span's `parent_id` must equal the client span's `span_id`:

```
Client span:  trace_id=abc123  span_id=def456  parent_id=none (root)
Server span:  trace_id=abc123  span_id=789ghi  parent_id=def456
                       ^^^^^^                            ^^^^^^
                    same trace                   matches client span_id
```

### 2. Check log correlation

Log records emitted during request handling should carry the server span's `trace_id` and `span_id`:

```
Log record:   trace_id=abc123  span_id=789ghi  body="Received request"
                       ^^^^^^          ^^^^^^
                    same trace    matches server span
```

### 3. Check span attributes

Both spans should have gRPC-specific attributes:
- `rpc.system` = `grpc`
- `rpc.service` = `helloworld.Greeter`
- `rpc.method` = `SayHello`
- Client span: `SpanKind.CLIENT`
- Server span: `SpanKind.SERVER`

### 4. Watch for zeroes

If you see `trace_id=0` or `trace_id=00000000000000000000000000000000`, the SDK is not configured correctly. See the troubleshooting section in the relevant language reference.

## Common Design Decisions

### Why traces + logs, not metrics?

Traces and correlated logs provide the most immediate value for debugging distributed systems. Metrics add complexity (meter providers, periodic readers, metric instruments) and are better added incrementally after traces and logs are working.

### Why W3C Trace Context?

It's the industry standard propagation format. All three OTel SDKs default to it. It works across language boundaries -- a Python client can trace into a Go server can trace into a Rust service, all linked by the same `traceparent` header in gRPC metadata.

### Why env-var-driven configuration?

Exporter selection, service naming, and batch tuning all happen via `OTEL_*` environment variables. The same binary can run in dev (`none` exporter), verification (`console` exporter), and production (`otlp` exporter) with zero code changes. Ops teams can tune telemetry without developer involvement.

### Why launch scripts?

Always create both a bash script (for Linux) and a `.cmd` script (for Windows) for each service. This keeps configuration out of application code and makes it easy to switch between dev/verification/production modes. Each reference file includes a launch script example -- use it as a starting point and create `run_server.sh` + `run_server.cmd` (and similarly for clients) in the project root. On Unix, use `exec` to replace the shell process with the application so signals propagate correctly.

### Why `none` in development?

The `none` exporter disables telemetry output, keeping stdout clean during normal development. Switch to `console` when you need to verify that tracing and log correlation are working correctly -- console exporters print structured telemetry to stdout so you can see exactly what the SDK produces, including span linkage, trace IDs, and attributes. Once verified, switch back to `none` for development or `otlp` for production.

### Why explicit shutdown?

Batch processors buffer telemetry in memory and flush periodically. If the process exits without flushing, buffered data is lost. All three languages ensure shutdown is called: Python via atexit, Go via `defer shutdown(ctx)`, Rust via `TelemetryGuard` with `Drop`. For short-lived processes (like gRPC clients), this is especially critical -- without it, spans may never be exported.
