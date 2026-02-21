# Async gRPC + OpenTelemetry in Python: A Complete Guide

This project demonstrates distributed tracing and log/trace correlation for an async Python gRPC application using OpenTelemetry's programmatic auto-instrumentation. All telemetry is exported to the console so you can see exactly what the SDK produces.

## What This Project Does

A gRPC `Greeter` service with a single `SayHello` RPC. A client sends a request; the server responds. OpenTelemetry instruments both sides, producing:

1. **Distributed traces** — a CLIENT span on the caller, a SERVER span on the callee, linked by W3C Trace Context propagated through gRPC metadata.
2. **Correlated logs** — Python `logging` output enriched with `trace_id`, `span_id`, and `service.name` so every log line can be joined to its trace.
3. **OTel log records** — Python log records bridged into the OpenTelemetry Logs pipeline and exported as structured JSON alongside spans.

## Project Structure

```
python/
├── protos/
│   └── helloworld.proto           # gRPC service definition
├── server.py                      # Async gRPC server
├── client.py                      # Async gRPC client
├── run_server.sh                  # Launch script with OTEL_* env vars
├── run_client.sh                  # Launch script with OTEL_* env vars
├── generate_protos.sh             # Proto compilation script
├── pyproject.toml                 # Python project config & dependencies
├── helloworld_pb2.py              # (generated)
├── helloworld_pb2_grpc.py         # (generated)
└── helloworld_pb2.pyi             # (generated)
```

## Quick Start

```bash
cd python

# Install dependencies
uv sync --group dev

# Generate protobuf code
chmod +x generate_protos.sh run_server.sh run_client.sh
./generate_protos.sh

# Terminal 1: start the server
./run_server.sh

# Terminal 2: send a request
./run_client.sh
```

---

## Dependencies

Requires **Python 3.11+** and **[uv](https://docs.astral.sh/uv/)** for dependency management. Run `uv sync --group dev` to install everything.

The dependencies are defined in `pyproject.toml`. OpenTelemetry's Python ecosystem is modular — there is no single "install opentelemetry" package. Each component has a distinct role, and omitting any one of them causes a specific failure mode. They break down into four layers:

### gRPC

| Package | Group | Why |
|---|---|---|
| `grpcio` | runtime | The async gRPC runtime. Provides `grpc.aio.server()` and `grpc.aio.insecure_channel()` that the application code uses directly. |
| `grpcio-tools` | dev | The `protoc` compiler and Python codegen plugin. Only needed to run `generate_protos.sh` — it produces `helloworld_pb2.py` and `helloworld_pb2_grpc.py` from the `.proto` file. Not imported at runtime, so it lives in the `dev` dependency group. |

### OpenTelemetry Core

| Package | Why |
|---|---|
| `opentelemetry-api` | The public API surface: `trace.get_tracer()`, `context`, propagators. All instrumentation code programs against this. By itself it's a no-op — every API call returns a non-recording stub unless an SDK is installed behind it. |
| `opentelemetry-sdk` | The concrete SDK: `TracerProvider`, `BatchSpanProcessor`, `ConsoleSpanExporter`, `LoggerProvider`, `BatchLogRecordProcessor`, `ConsoleLogExporter`. This is what actually records spans and log records and exports them. Without it, the API layer does nothing. |

### Auto-Instrumentation

| Package | Why |
|---|---|
| `opentelemetry-instrumentation` | Provides the `initialize()` function that discovers and activates all installed instrumentors via entry points. This is the single call at the top of `server.py` and `client.py` that sets everything up. |
| `opentelemetry-instrumentation-grpc` | Registers `GrpcInstrumentorServer` and `GrpcInstrumentorClient` as entry points. When `initialize()` discovers them, they monkey-patch `grpc.aio.server()` and `grpc.aio.insecure_channel()` to automatically wrap calls with tracing interceptors. Without this, gRPC calls produce no spans. |
| `opentelemetry-instrumentation-logging` | Registers `LoggingInstrumentor` as an entry point. When `initialize()` discovers it, it patches Python's `logging` module to inject `otelTraceID`, `otelSpanID`, and `otelServiceName` into every `LogRecord`. Without this, log lines have no trace context. |

### Distribution

| Package | Why |
|---|---|
| `opentelemetry-distro` | Registers `OpenTelemetryConfigurator` as an entry point. The configurator reads `OTEL_*` env vars and builds the SDK — creating a `TracerProvider` with the right exporter, a `LoggerProvider`, and propagators. **This is the most subtle dependency.** Without it, `initialize()` still discovers and activates all the instrumentors (monkey-patching happens normally), but no SDK is configured. The result: every span is a `NonRecordingSpan` with `trace_id=0`, every log shows `[trace_id=0 span_id=0]`, and nothing is exported. It *looks* like instrumentation is working because the format is there, but the zeroes mean nothing is actually traced. See [Gotcha #5](#5-opentelemetry-distro-is-essential-but-easy-to-forget) for the full breakdown. |

For a detailed view of how these packages interact during `initialize()`, see [The Dependency Chain](#the-dependency-chain) and [initialize-internals.md](initialize-internals.md).

---

## How It Works (and Why It's Done This Way)

### The Critical Import Order

The single most important thing in this project is the first two lines of `server.py` and `client.py`:

```python
from opentelemetry.instrumentation.auto_instrumentation import initialize
initialize()
```

**These lines MUST come before `import grpc`** (or any other library you want instrumented). Here's why:

`initialize()` does three things in sequence:

1. **Runs the Configurator** (from `opentelemetry-distro`). This reads `OTEL_*` environment variables and creates a `TracerProvider` with the appropriate exporter (e.g., `ConsoleSpanExporter` when `OTEL_TRACES_EXPORTER=console`), a `LoggerProvider` with its exporter, and propagators. It calls `trace.set_tracer_provider()` and `set_global_textmap()` to install them globally.

2. **Discovers all installed instrumentors** by scanning `opentelemetry_instrumentor` entry points in installed packages. In our case it finds `GrpcInstrumentorServer`, `GrpcInstrumentorClient`, and `LoggingInstrumentor`.

3. **Calls `.instrument()` on each one.** The gRPC instrumentors monkey-patch `grpc.aio.server()` and `grpc.aio.insecure_channel()` to automatically wrap them with tracing interceptors. The `LoggingInstrumentor` patches Python's `logging` module to inject `otelTraceID`, `otelSpanID`, `otelServiceName`, and `otelTraceSampled` into every log record's format string.

If you import `grpc` before calling `initialize()`, the module-level references are captured before the monkey-patching happens, and you get uninstrumented gRPC with no spans.

### Why `logging.basicConfig()` Comes Later (and Still Works)

```python
# In __main__:
logging.basicConfig(level=logging.INFO)
```

The `LoggingInstrumentor.instrument()` call (triggered by `initialize()`) modifies `logging.basicConfig` itself — it replaces the default format string with one that includes `[trace_id=%(otelTraceID)s span_id=%(otelSpanID)s resource.service.name=%(otelServiceName)s trace_sampled=%(otelTraceSampled)s]`. When we later call `logging.basicConfig(level=logging.INFO)` without specifying a `format=`, the instrumented version's format takes effect.

The result is that every log line emitted via Python `logging` automatically includes trace context:

```
2026-02-21 18:21:47,412 INFO [__main__] [server.py:18] [trace_id=b73187073037d9521d204e1593b0000b span_id=177f6a5e28fd8863 resource.service.name=grpc-server trace_sampled=True] - Received request: name=World
```

When there is no active span (e.g., at server startup), the fields show zeroes:

```
[trace_id=0 span_id=0 resource.service.name=grpc-server trace_sampled=False]
```

---

## Environment Variables: What Each One Does

The launch scripts (`run_server.sh`, `run_client.sh`) set these environment variables. They're read by the `OpenTelemetryConfigurator` (from `opentelemetry-distro`) during `initialize()`.

### Core Configuration

| Variable | Value | What It Does |
|---|---|---|
| `OTEL_SERVICE_NAME` | `grpc-server` / `grpc-client` | Sets `resource.service.name` on every span and log record. This is how you identify which service produced the telemetry. Appears in log correlation fields as `otelServiceName`. |
| `OTEL_TRACES_EXPORTER` | `console` | Uses `ConsoleSpanExporter`, which prints each completed span as pretty-printed JSON to stdout. In production you'd use `otlp` to send to a collector. |
| `OTEL_LOGS_EXPORTER` | `console` | Uses `ConsoleLogExporter`, which prints each OTel log record as JSON to stdout. Only relevant when `OTEL_PYTHON_LOGGING_AUTO_INSTRUMENTATION_ENABLED=true`. |
| `OTEL_METRICS_EXPORTER` | `none` | Disables metric export entirely. Without this, the SDK installs a `PeriodicExportingMetricReader` that periodically dumps metric data to stdout, which clutters the output. |
| `OTEL_PROPAGATORS` | `tracecontext,baggage` | Registers W3C Trace Context and Baggage propagators. The gRPC client interceptor calls `inject()` to write `traceparent`/`tracestate` into gRPC metadata; the server interceptor calls `extract()` to read them back. This is what creates the parent-child relationship between client and server spans. |

### Python-Specific Configuration

| Variable | Value | What It Does |
|---|---|---|
| `OTEL_PYTHON_LOG_CORRELATION` | `true` | Tells `LoggingInstrumentor` to inject `otelTraceID`, `otelSpanID`, `otelTraceSampled`, and `otelServiceName` attributes into Python `LogRecord` objects, and to modify the default `basicConfig` format string to include them. This is what produces the `[trace_id=... span_id=...]` fields in stderr log output. |
| `OTEL_PYTHON_LOG_LEVEL` | `info` | Sets the root logger level during `initialize()`. Controls what severity of log records are emitted. |
| `OTEL_PYTHON_LOGGING_AUTO_INSTRUMENTATION_ENABLED` | `true` | Tells the configurator to attach an `opentelemetry.sdk._logs.LoggingHandler` to the Python root logger. This bridges every Python `logging` call into the OTel Logs pipeline — the log record gets converted into an OTel `LogRecord`, processed by the `BatchLogRecordProcessor`, and exported by the `ConsoleLogExporter`. Without this, you only get trace/span correlation in stderr log lines but no OTel log records. |

### Batch Processor Tuning

| Variable | Value | What It Does |
|---|---|---|
| `OTEL_BSP_SCHEDULE_DELAY` | `1` | `BatchSpanProcessor` schedule delay in milliseconds (default: 5000). The BSP accumulates completed spans in a queue and exports them in batches on a background thread timer. Setting to `1` means spans appear in console output within ~1ms of completion instead of up to 5 seconds later. Essential for interactive debugging; use the default in production. |
| `OTEL_BLRP_SCHEDULE_DELAY` | `1` | `BatchLogRecordProcessor` schedule delay in milliseconds (default: 5000). Same concept as BSP but for OTel log records. Without this, log records emitted during a request might not appear in stdout until 5 seconds later, which is confusing when watching console output. |

---

## Exporting via OTLP Instead of Console

The `console` exporter is useful for seeing raw telemetry during development, but in a real setup you'll send telemetry to a collector or observability backend via OTLP (OpenTelemetry Protocol). Since `opentelemetry-distro` already includes `opentelemetry-exporter-otlp` as a dependency, no additional packages are needed — just change the environment variables.

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

By default, the OTLP exporter sends telemetry via gRPC to `http://localhost:4317`. If you have an OpenTelemetry Collector or a compatible backend (Jaeger, Grafana Tempo, etc.) listening there, spans and log records will start flowing immediately.

### OTLP Configuration Variables

| Variable | Default | What It Does |
|---|---|---|
| `OTEL_EXPORTER_OTLP_ENDPOINT` | `http://localhost:4317` (gRPC) or `http://localhost:4318` (HTTP) | The collector endpoint. Set this to point at your collector or backend. |
| `OTEL_EXPORTER_OTLP_PROTOCOL` | `grpc` | Protocol to use. `grpc` sends to port 4317; `http/protobuf` sends to port 4318. |
| `OTEL_EXPORTER_OTLP_HEADERS` | *(none)* | Comma-separated `key=value` pairs sent as headers on every export request. Used for authentication tokens (e.g., `x-api-key=secret`). |
| `OTEL_EXPORTER_OTLP_CERTIFICATE` | *(none)* | Path to a TLS certificate file for the collector connection. |

You can also set signal-specific overrides like `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` or `OTEL_EXPORTER_OTLP_LOGS_ENDPOINT` if traces and logs go to different destinations.

### Example: Sending to a Local Collector

```bash
export OTEL_SERVICE_NAME="grpc-server"
export OTEL_TRACES_EXPORTER="otlp"
export OTEL_LOGS_EXPORTER="otlp"
export OTEL_METRICS_EXPORTER="none"
export OTEL_EXPORTER_OTLP_ENDPOINT="http://localhost:4317"
export OTEL_PROPAGATORS="tracecontext,baggage"
export OTEL_PYTHON_LOG_CORRELATION="true"
export OTEL_PYTHON_LOG_LEVEL="info"
export OTEL_PYTHON_LOGGING_AUTO_INSTRUMENTATION_ENABLED="true"
```

Note that `OTEL_PYTHON_LOG_CORRELATION=true` still works with OTLP — it controls the stderr log format (injecting `trace_id`/`span_id` into Python `logging` output), which is independent of where OTel spans and log records are exported.

---

## How Distributed Tracing Works Across gRPC

Here's the exact flow when the client calls `SayHello`:

### 1. Client creates a span

The `GrpcInstrumentorClient` patched `grpc.aio.insecure_channel()` to return a channel wrapped with an `OpenTelemetryClientInterceptor`. When `stub.SayHello()` is called, the interceptor:

- Creates a new span with `kind=SpanKind.CLIENT` and name `/helloworld.Greeter/SayHello`
- Sets attributes: `rpc.system=grpc`, `rpc.service=helloworld.Greeter`, `rpc.method=SayHello`
- Calls `inject(carrier=metadata)` on the global propagator, which writes a `traceparent` header into the gRPC metadata:
  ```
  traceparent: 00-b73187073037d9521d204e1593b0000b-11c6b6f5eb9047a3-01
  ```
  (version-traceId-spanId-flags)

### 2. Server extracts the context and creates a child span

The `GrpcInstrumentorServer` patched `grpc.aio.server()` to add an `OpenTelemetryServerInterceptor`. When the request arrives, the interceptor:

- Calls `extract(carrier=metadata)` on the global propagator, which parses the `traceparent` header and reconstructs the remote `SpanContext`
- Creates a new span with `kind=SpanKind.SERVER`, setting the extracted context as the parent
- The server span gets the **same `trace_id`** as the client span, and its `parent_id` is set to the client span's `span_id`

### 3. Log correlation happens automatically

Inside the `SayHello` handler, the server span is the active span. When `logger.info("Received request: ...")` is called:

- `LoggingInstrumentor` has patched `LogRecord` creation to inject `otelTraceID` and `otelSpanID` from the current active span
- The stderr output shows the trace context in the formatted log line
- If `OTEL_PYTHON_LOGGING_AUTO_INSTRUMENTATION_ENABLED=true`, the `LoggingHandler` on the root logger also converts this into an OTel `LogRecord` with `trace_id` and `span_id` fields, which gets exported as JSON

### 4. Verification: matching IDs

In the console output, you can verify the distributed trace by matching IDs:

```
# Client span:
"trace_id": "0xb73187073037d9521d204e1593b0000b"
"span_id": "0x11c6b6f5eb9047a3"
"kind": "SpanKind.CLIENT"
"parent_id": null                                    # root span

# Server span:
"trace_id": "0xb73187073037d9521d204e1593b0000b"     # same trace!
"span_id": "0x177f6a5e28fd8863"
"kind": "SpanKind.SERVER"
"parent_id": "0x11c6b6f5eb9047a3"                    # matches client span_id

# Server log line (stderr):
[trace_id=b73187073037d9521d204e1593b0000b span_id=177f6a5e28fd8863 ...]

# Server OTel log record (stdout JSON):
"trace_id": "0xb73187073037d9521d204e1593b0000b"     # same trace as both spans
"span_id": "0x177f6a5e28fd8863"                      # matches server span
```

---

## Three Kinds of Telemetry Output

With this configuration, you see three distinct types of output:

### 1. Python logging to stderr

Standard `logging` module output, enhanced with trace context by `LoggingInstrumentor`:

```
2026-02-21 18:21:47,412 INFO [__main__] [server.py:18] [trace_id=b73187073037d9521d204e1593b0000b span_id=177f6a5e28fd8863 resource.service.name=grpc-server trace_sampled=True] - Received request: name=World
```

Controlled by: `OTEL_PYTHON_LOG_CORRELATION=true`

### 2. OTel spans to stdout (JSON)

`ConsoleSpanExporter` output — one JSON object per completed span:

```json
{
    "name": "/helloworld.Greeter/SayHello",
    "context": {
        "trace_id": "0xb73187073037d9521d204e1593b0000b",
        "span_id": "0x177f6a5e28fd8863"
    },
    "kind": "SpanKind.SERVER",
    "parent_id": "0x11c6b6f5eb9047a3",
    "attributes": {
        "rpc.system": "grpc",
        "rpc.method": "SayHello",
        "rpc.service": "helloworld.Greeter",
        "rpc.grpc.status_code": 0
    },
    "resource": {
        "attributes": {
            "service.name": "grpc-server"
        }
    }
}
```

Controlled by: `OTEL_TRACES_EXPORTER=console`

### 3. OTel log records to stdout (JSON)

`ConsoleLogExporter` output — one JSON object per Python log call, bridged through the OTel Logs pipeline:

```json
{
    "body": "Received request: name=World",
    "severity_number": 9,
    "severity_text": "INFO",
    "attributes": {
        "otelSpanID": "177f6a5e28fd8863",
        "otelTraceID": "b73187073037d9521d204e1593b0000b",
        "otelServiceName": "grpc-server",
        "code.file.path": "/path/to/server.py",
        "code.function.name": "SayHello",
        "code.line.number": 18
    },
    "trace_id": "0xb73187073037d9521d204e1593b0000b",
    "span_id": "0x177f6a5e28fd8863"
}
```

Controlled by: `OTEL_LOGS_EXPORTER=console` + `OTEL_PYTHON_LOGGING_AUTO_INSTRUMENTATION_ENABLED=true`

---

## Package Roles

Each package in `pyproject.toml` has a specific job:

| Package | Role |
|---|---|
| `grpcio` | The async gRPC runtime (`grpc.aio.server()`, `grpc.aio.insecure_channel()`) |
| `grpcio-tools` | `protoc` compiler + Python codegen plugin; used by `generate_protos.sh` |
| `opentelemetry-api` | Public API: `trace.get_tracer()`, `context`, propagators. No-op by default until an SDK is configured. |
| `opentelemetry-sdk` | Concrete implementations: `TracerProvider`, `BatchSpanProcessor`, `ConsoleSpanExporter`, `LoggerProvider`, `BatchLogRecordProcessor`, `ConsoleLogExporter`. This is what actually records and exports telemetry. |
| `opentelemetry-instrumentation` | Provides `initialize()` — the entry point that discovers and activates all installed instrumentors. Also provides the base `BaseInstrumentor` class. |
| `opentelemetry-instrumentation-grpc` | Registers `GrpcInstrumentorServer` and `GrpcInstrumentorClient` as entry points. When `initialize()` discovers them, they monkey-patch `grpc.aio.server()` and `grpc.aio.insecure_channel()` to automatically add tracing interceptors. |
| `opentelemetry-instrumentation-logging` | Registers `LoggingInstrumentor` as an entry point. When `initialize()` discovers it, it patches Python's `logging` to inject trace context (`otelTraceID`, `otelSpanID`, etc.) into every `LogRecord`. |
| `opentelemetry-distro` | Registers `OpenTelemetryDistro` and `OpenTelemetryConfigurator` as entry points. The configurator is what reads `OTEL_*` env vars and sets up `TracerProvider`/`LoggerProvider` with the correct exporters and propagators. Without this package, `initialize()` would discover instrumentors but have no SDK configured — so all instrumentation would be no-ops. |

### The Dependency Chain

```
initialize()
  ├── OpenTelemetryConfigurator.configure()     [from opentelemetry-distro]
  │     ├── reads OTEL_TRACES_EXPORTER → creates ConsoleSpanExporter
  │     ├── reads OTEL_LOGS_EXPORTER → creates ConsoleLogExporter
  │     ├── reads OTEL_PROPAGATORS → sets up W3C TraceContext + Baggage
  │     ├── creates TracerProvider with BatchSpanProcessor
  │     ├── creates LoggerProvider with BatchLogRecordProcessor
  │     └── calls trace.set_tracer_provider() / sets global propagator
  │
  ├── GrpcInstrumentorServer.instrument()       [from opentelemetry-instrumentation-grpc]
  │     └── monkey-patches grpc.aio.server() to add server interceptor
  │
  ├── GrpcInstrumentorClient.instrument()       [from opentelemetry-instrumentation-grpc]
  │     └── monkey-patches grpc.aio.insecure_channel() to add client interceptor
  │
  └── LoggingInstrumentor.instrument()          [from opentelemetry-instrumentation-logging]
        ├── patches LogRecord to inject otelTraceID/otelSpanID
        └── modifies basicConfig default format to include trace fields
```

---

## Gotchas and Lessons Learned

### 1. Import order is non-negotiable

```python
# CORRECT:
from opentelemetry.instrumentation.auto_instrumentation import initialize
initialize()
import grpc  # gets the monkey-patched version

# WRONG:
import grpc  # captures un-patched module references
from opentelemetry.instrumentation.auto_instrumentation import initialize
initialize()  # too late — grpc module already imported
```

### 2. `OTEL_METRICS_EXPORTER=none` prevents noisy metric dumps

Without disabling the metrics exporter, the SDK sets up a `PeriodicExportingMetricReader` that dumps metric data (gRPC instrumentation produces various RPC metrics) to stdout every 60 seconds. This interleaves with your span and log output and is confusing during development.

### 3. Batch processor delays hide output during development

The `BatchSpanProcessor` and `BatchLogRecordProcessor` both default to a 5-second schedule delay. This means after a request completes, you might wait up to 5 seconds before seeing the span or log record in stdout. Set `OTEL_BSP_SCHEDULE_DELAY=1` and `OTEL_BLRP_SCHEDULE_DELAY=1` (1 millisecond) for near-instant output during development.

In production, the defaults are fine — batching amortizes export overhead.

### 4. Two separate log correlation mechanisms

There are two independent features that are easy to conflate:

- **`OTEL_PYTHON_LOG_CORRELATION=true`** — injects trace context fields into Python `LogRecord` objects and modifies the `basicConfig` format. This affects the *stderr* output from the standard `logging` module. You get human-readable log lines with `[trace_id=... span_id=...]`.

- **`OTEL_PYTHON_LOGGING_AUTO_INSTRUMENTATION_ENABLED=true`** — attaches an OTel `LoggingHandler` to the root logger that bridges Python log records into the OTel Logs pipeline. This produces *structured JSON* OTel log records on stdout via `ConsoleLogExporter`. These records have proper `trace_id` and `span_id` fields that an observability backend can use for correlation.

You probably want both enabled: the stderr output for human reading, and the OTel log records for machine consumption.

### 5. `opentelemetry-distro` is essential but easy to forget

`initialize()` has four steps internally:

```
1. distro = _load_distro()      # load a distribution
2. distro.configure()           # let it set env var defaults
3. _load_configurators()        # set up the SDK (providers, exporters, processors)
4. _load_instrumentors(distro)  # monkey-patch libraries (grpc, logging, etc.)
```

Steps 1, 2, and 4 work fine without `opentelemetry-distro`. The instrumentors are discovered from their own packages (`opentelemetry-instrumentation-grpc`, `opentelemetry-instrumentation-logging`) and monkey-patching happens normally. But **step 3 does nothing** — `_load_configurators()` scans the `opentelemetry_configurator` entry point group, finds no registered configurators, and returns silently.

The configurator is the component that reads your `OTEL_*` env vars and creates the SDK objects:
- `TracerProvider` with `BatchSpanProcessor` and your chosen exporter (e.g., `ConsoleSpanExporter`)
- `LoggerProvider` with `BatchLogRecordProcessor` and your chosen log exporter
- Registers these globally via `set_tracer_provider()` / `set_logger_provider()`

Without it, the global tracer provider remains a `ProxyTracerProvider` that delegates to `NoOpTracer`. The gRPC interceptors still run on every request, but `tracer.start_span()` returns a `NonRecordingSpan` with trace_id=0 and span_id=0. Spans are silently discarded. The logging patches still inject `otelTraceID` and `otelSpanID` into every log record, but the values are always `0`. No `LoggingHandler` is attached, so no OTel log records are produced.

The result is deceptive: your log lines have `[trace_id=0 span_id=0 resource.service.name=... ]` — the format *looks* like instrumentation is working, but the zeroes mean nothing is actually being traced. No spans are exported, no logs are correlated, no distributed traces link client to server.

`opentelemetry-distro` provides both `OpenTelemetryDistro` (which sets OTLP exporter defaults via `os.environ.setdefault`) and `OpenTelemetryConfigurator` (which actually builds the SDK). The configurator is the critical piece — it's the glue between "I set `OTEL_TRACES_EXPORTER=console`" and "I see spans on stdout."

For a deeper analysis of the `initialize()` code path, see [initialize-internals.md](initialize-internals.md).

### 6. `logging.basicConfig()` is a no-op when handlers already exist

Python's `logging.basicConfig()` only takes effect if the root logger has no handlers. Since `initialize()` may add handlers (via `LoggingInstrumentor` and optionally via `OTEL_PYTHON_LOGGING_AUTO_INSTRUMENTATION_ENABLED`), a subsequent `basicConfig()` call might be a no-op. In this project it still works because `LoggingInstrumentor` patches `basicConfig` itself to inject the trace-correlated format. But be aware of this interaction if you customize your logging setup.

### 7. The server and client share the same trace_id

The whole point of distributed tracing is that a single trace_id follows a request across service boundaries. The client span and server span both carry the same `trace_id`. The server span's `parent_id` equals the client span's `span_id`. This linkage is established through W3C Trace Context propagation via gRPC metadata — the `traceparent` header carries `traceId-spanId-traceFlags` from client to server.
