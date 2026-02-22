# OpenTelemetry + gRPC in Python

## Table of Contents

1. [Dependencies](#dependencies)
2. [How It Works](#how-it-works)
3. [Example Code](#example-code)
4. [Launch Scripts](#launch-scripts)
5. [Environment Variables](#environment-variables)
6. [Testing That It Works](#testing-that-it-works)
7. [Cancelling Server-Streaming Responses](#cancelling-server-streaming-responses)
8. [Troubleshooting](#troubleshooting)

---

## Dependencies

```toml
# pyproject.toml
[project]
dependencies = [
    "grpcio>=1.68.0",
    "opentelemetry-api",
    "opentelemetry-sdk",
    "opentelemetry-instrumentation",
    "opentelemetry-instrumentation-grpc",
    "opentelemetry-instrumentation-logging",
    "opentelemetry-distro",
]

[dependency-groups]
dev = [
    "grpcio-tools>=1.68.0",
]
```

### What each package does

| Package | Role |
|---------|------|
| `grpcio` | Async gRPC runtime (`grpc.aio.server()`, `grpc.aio.insecure_channel()`) |
| `grpcio-tools` | protoc compiler + Python codegen plugin (dev only) |
| `opentelemetry-api` | Public API surface. No-op by itself until an SDK is configured behind it. |
| `opentelemetry-sdk` | Concrete SDK: `TracerProvider`, `BatchSpanProcessor`, `ConsoleSpanExporter`, `LoggerProvider`, `BatchLogRecordProcessor`, `ConsoleLogExporter`. This is what actually records and exports telemetry. |
| `opentelemetry-instrumentation` | Provides the `initialize()` function that discovers and activates all instrumentors via entry points. |
| `opentelemetry-instrumentation-grpc` | Registers `GrpcInstrumentorServer` and `GrpcInstrumentorClient`. When `initialize()` discovers them, they monkey-patch `grpc.aio.server()` and `grpc.aio.insecure_channel()` to add tracing interceptors. |
| `opentelemetry-instrumentation-logging` | Registers `LoggingInstrumentor`. Patches Python's `logging` to inject `otelTraceID`, `otelSpanID`, `otelServiceName` into every `LogRecord`. |
| `opentelemetry-distro` | **The most subtle dependency.** Registers `OpenTelemetryConfigurator` which reads `OTEL_*` env vars and builds the SDK. Without it, all instrumentors still activate (monkey-patching happens), but no SDK is configured -- every span is a `NonRecordingSpan` with `trace_id=0`, nothing is exported. See [Troubleshooting](#1-all-trace-ids-are-zero). |

### gRPC version constraints

The lower bounds for `grpcio` and `grpcio-tools` in `pyproject.toml` must match the version of `grpcio-tools` used to generate the proto bindings. The generated gRPC stub files (`*_pb2_grpc.py`) contain a `GRPC_GENERATED_VERSION` constant (e.g., `'1.78.1'`). At import time, the stub compares the installed `grpc.__version__` against this value and raises a `RuntimeError` if the installed version is lower.

When regenerating proto bindings:

1. Check `GRPC_GENERATED_VERSION` in all `*_pb2_grpc.py` files.
2. Find the highest version among them.
3. Update `pyproject.toml` so the lower bounds for `grpcio` (and `grpcio-tools` in dev dependencies) are `>=` that highest version.

---

## How It Works

Python uses **programmatic auto-instrumentation**. The application code has zero OTel API calls -- no `get_tracer()`, no `start_span()`, no manual context propagation. Everything is automatic.

The setup relies on a single critical call:

```python
from opentelemetry.instrumentation.auto_instrumentation import initialize
initialize()
```

`initialize()` does four things in sequence:

1. **Loads the distribution** (`opentelemetry-distro`) -- sets default OTLP env vars if not already set.
2. **Runs the configurator** -- reads `OTEL_*` env vars, creates `TracerProvider` with the right exporter, a `LoggerProvider`, and propagators. Registers them globally.
3. **Discovers all instrumentors** by scanning entry points -- finds `GrpcInstrumentorServer`, `GrpcInstrumentorClient`, and `LoggingInstrumentor`.
4. **Calls `.instrument()` on each** -- the gRPC instrumentors monkey-patch `grpc.aio.server()` and `grpc.aio.insecure_channel()` to automatically wrap them with tracing interceptors. The `LoggingInstrumentor` patches `logging` to inject trace context fields.

### Why the import order matters

`initialize()` and `import grpc` must happen in exactly that order. The gRPC instrumentors work by monkey-patching module-level references in `grpc.aio`. If `grpc` is imported first, those references are captured before patching occurs, resulting in uninstrumented gRPC with no spans.

```python
# CORRECT:
from opentelemetry.instrumentation.auto_instrumentation import initialize
initialize()
import grpc  # gets the monkey-patched version

# WRONG -- gRPC will be uninstrumented:
import grpc  # captures un-patched references
from opentelemetry.instrumentation.auto_instrumentation import initialize
initialize()  # too late
```

### Two independent log correlation mechanisms

Python provides two separate features that are easy to conflate:

1. **`OTEL_PYTHON_LOG_CORRELATION=true`** -- injects `otelTraceID`, `otelSpanID`, etc. into Python `LogRecord` objects and modifies the `basicConfig` format to include them. This affects **stderr output** from the standard `logging` module. You get human-readable log lines with `[trace_id=... span_id=...]`.

2. **`OTEL_PYTHON_LOGGING_AUTO_INSTRUMENTATION_ENABLED=true`** -- attaches an OTel `LoggingHandler` to the root logger that bridges Python log records into the OTel Logs pipeline. This produces **structured JSON** OTel log records on stdout. These records have proper `trace_id` and `span_id` fields for backend correlation.

Enable both: stderr for humans, OTel log records for machines.

---

## Example Code

```python
from opentelemetry.instrumentation.auto_instrumentation import initialize
initialize()

import asyncio
import logging

import grpc

import helloworld_pb2
import helloworld_pb2_grpc

logger = logging.getLogger(__name__)

class GreeterServicer(helloworld_pb2_grpc.GreeterServicer):
    async def SayHello(self, request, context):
        logger.info("Received request: name=%s", request.name)
        return helloworld_pb2.HelloReply(message=f"Hello, {request.name}!")


async def serve():
    server = grpc.aio.server()
    helloworld_pb2_grpc.add_GreeterServicer_to_server(GreeterServicer(), server)
    server.add_insecure_port("[::]:50051")
    logger.info("Server starting on port 50051")
    await server.start()
    logger.info("Server started")
    await server.wait_for_termination()


async def client():
    async with grpc.aio.insecure_channel("localhost:50051") as channel:
        stub = helloworld_pb2_grpc.GreeterStub(channel)
        response = await stub.SayHello(helloworld_pb2.HelloRequest(name="World"))
        logger.info("Greeter response: %s", response.message)


if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO)
    asyncio.run(serve())  # or asyncio.run(client())
```

Key points:
- `logging.basicConfig(level=logging.INFO)` uses the trace-correlated format because `LoggingInstrumentor` patched `basicConfig` during `initialize()`
- Uses `grpc.aio` (async API), the recommended approach for new gRPC Python applications

---

## Launch Scripts

```bash
export OTEL_SERVICE_NAME="grpc-server"  # or "grpc-client"
export OTEL_TRACES_EXPORTER="none"
export OTEL_LOGS_EXPORTER="none"
export OTEL_METRICS_EXPORTER="none"
export OTEL_PROPAGATORS="tracecontext,baggage"
export OTEL_PYTHON_LOG_CORRELATION="true"
export OTEL_PYTHON_LOG_LEVEL="info"
export OTEL_PYTHON_LOGGING_AUTO_INSTRUMENTATION_ENABLED="true"
export OTEL_BSP_SCHEDULE_DELAY="1"
export OTEL_BLRP_SCHEDULE_DELAY="1"

exec python example.py
```

---

## Environment Variables

### Python-Specific Variables

These are unique to the Python OTel SDK:

| Variable | Value | Purpose |
|----------|-------|---------|
| `OTEL_METRICS_EXPORTER` | `none` | **Must be set to `none`** or the SDK installs a `PeriodicExportingMetricReader` that dumps gRPC RPC metrics to stdout every 60 seconds, interleaving with span/log output. |
| `OTEL_PYTHON_LOG_CORRELATION` | `true` | Injects trace context into Python `LogRecord` objects and modifies `basicConfig` format. Controls **stderr** log output. |
| `OTEL_PYTHON_LOG_LEVEL` | `info` | Sets root logger level during `initialize()`. |
| `OTEL_PYTHON_LOGGING_AUTO_INSTRUMENTATION_ENABLED` | `true` | Bridges Python `logging` into OTel Logs pipeline. Controls **stdout** OTel log records. |

See SKILL.md for common `OTEL_*` environment variables.

---

## Testing That It Works

See SKILL.md §"How to Verify It Works" for the general process (trace linkage, log correlation). Python produces three types of output:

**1. Python logging to stderr (trace-correlated):**
```
2026-02-21 18:21:47,412 INFO [__main__] [server.py:18] [trace_id=b73187073037d9521d204e1593b0000b span_id=177f6a5e28fd8863 resource.service.name=grpc-server trace_sampled=True] - Received request: name=World
```

**2. OTel spans to stdout (JSON):**
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
        "rpc.service": "helloworld.Greeter"
    }
}
```

**3. OTel log records to stdout (JSON):**
```json
{
    "body": "Received request: name=World",
    "severity_text": "INFO",
    "trace_id": "0xb73187073037d9521d204e1593b0000b",
    "span_id": "0x177f6a5e28fd8863"
}
```

---

## Cancelling Server-Streaming Responses

OpenTelemetry instrumentation wraps gRPC streams in an async generator, hiding the `.cancel()` method. Use this helper:

```python
async def cancel_stream(stream):
    if hasattr(stream, "cancel"):
        stream.cancel()
    else:
        await stream.aclose()
```

Usage:

```python
stream = stub.ServerStreamingMethod(request)
async for response in stream:
    if should_stop(response):
        await cancel_stream(stream)
        break
```

When the stream is instrumented, `aclose()` triggers `GeneratorExit` in the async generator, which hits the instrumentation's `finally: span.end()` block and cleanly ends the span.

This works around a bug in `opentelemetry-instrumentation-grpc` 0.60b1 and earlier ([opentelemetry-python-contrib#2014](https://github.com/open-telemetry/opentelemetry-python-contrib/issues/2014)). Remove the helper once an upstream fix is released ([PR #3823](https://github.com/open-telemetry/opentelemetry-python-contrib/pull/3823), [PR #2093](https://github.com/open-telemetry/opentelemetry-python-contrib/pull/2093)).

---

## Troubleshooting

### 1. All trace IDs are zero

**Symptom:** Log lines show `[trace_id=0 span_id=0 ...]`. No spans appear on stdout.

**Cause:** `opentelemetry-distro` is not installed. See the [dependency table](#what-each-package-does) for why this package is critical.

**Fix:** Add `opentelemetry-distro` to your dependencies and reinstall.

### 2. No spans appear, but logs work

**Symptom:** Python `logging` output appears on stderr, but no span JSON appears on stdout.

**Cause:** Import order is wrong — `grpc` was imported before `initialize()`. See [Why the import order matters](#why-the-import-order-matters).

**Fix:** Ensure `initialize()` is called before `import grpc`.

### 3. Periodic metric dumps cluttering stdout

**Symptom:** Every 60 seconds, a large block of metric data appears on stdout, interleaving with span/log output.

**Cause:** `OTEL_METRICS_EXPORTER` is not set to `none`. The SDK creates a `PeriodicExportingMetricReader` that dumps gRPC RPC metrics.

**Fix:** Add to your launch script:
```bash
export OTEL_METRICS_EXPORTER="none"
```

### 4. Spans appear only after 5 seconds

**Symptom:** After a request, spans and log records don't appear for up to 5 seconds.

**Cause:** `OTEL_BSP_SCHEDULE_DELAY` and `OTEL_BLRP_SCHEDULE_DELAY` are using defaults (5000ms).

**Fix:** Set both to `1` in your launch scripts for development:
```bash
export OTEL_BSP_SCHEDULE_DELAY="1"
export OTEL_BLRP_SCHEDULE_DELAY="1"
```

### 5. Client and server spans have different trace IDs

**Symptom:** Both client and server produce spans, but with different `trace_id` values.

**Cause:** Context propagation is broken — typically a combination of causes #1 and #2.

**Fix:** Verify `opentelemetry-distro` is installed, `OTEL_PROPAGATORS=tracecontext,baggage` is set, and `initialize()` comes before `import grpc`.

### 6. `AttributeError: 'async_generator' object has no attribute 'cancel'`

**Symptom:** Calling `.cancel()` on a server-streaming response raises `AttributeError`.

**Cause:** OTel gRPC instrumentation wraps the stream in an async generator, which lacks `.cancel()`.

**Fix:** Use the `cancel_stream()` helper from [Cancelling Server-Streaming Responses](#cancelling-server-streaming-responses).

### 7. `grpc.aio.AioRpcError` with `CANCELLED` after cancelling a stream

**Symptom:** After cancelling a stream, continuing the `async for` loop raises `AioRpcError(CANCELLED)`.

**Cause:** The behavior after cancelling depends on whether the stream is instrumented: a raw gRPC stream raises `AioRpcError(CANCELLED)` on the next iteration, while an instrumented async generator silently exits the loop (since `aclose()` closes the generator, causing `StopAsyncIteration`).

**Fix:** Wrap the loop in `try`/`except grpc.aio.AioRpcError` and check for `StatusCode.CANCELLED` to handle this consistently.

### 8. `logging.basicConfig()` seems to have no effect

**Symptom:** Log format doesn't include trace context, or log level isn't what you expect.

**Cause:** Python's `logging.basicConfig()` is a no-op if the root logger already has handlers. `initialize()` may have already added handlers.

**Fix:** In this setup, `LoggingInstrumentor` patches `basicConfig` to inject the trace-correlated format. If you're setting a custom format, set it *after* `initialize()` but know that the trace context injection in `basicConfig` won't apply -- you'll need to include `%(otelTraceID)s` and `%(otelSpanID)s` manually in your format string.
