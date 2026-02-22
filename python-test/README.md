# python-test: gRPC + OpenTelemetry in a Single Pytest

This folder demonstrates distributed tracing with gRPC and OpenTelemetry, verified in a single pytest that runs both server and client in-process.

## Why programmatic OTel setup instead of `initialize()` + env vars?

The `python/` folder uses `opentelemetry.instrumentation.auto_instrumentation.initialize()`, which reads `OTEL_*` env vars, uses `BatchSpanProcessor`, and relies on entry-point discovery. That approach works well for running services, but is awkward for testing:

- **`InMemorySpanExporter`** captures spans in memory for direct assertions — no stdout parsing needed.
- **`SimpleSpanProcessor`** exports spans synchronously, eliminating flush timing issues.
- Avoids the fragile "`initialize()` must be called before importing grpc" ordering constraint.

The gRPC instrumentors (`GrpcAioInstrumentorServer`/`Client`) still monkey-patch `grpc.aio` the same way, so the instrumentation mechanism is identical.

## Why fewer dependencies than `python/`?

Since we configure OTel programmatically, we don't need:
- `opentelemetry-distro` (provides `initialize()`)
- `opentelemetry-instrumentation` (entry-point-based auto-instrumentation)
- `opentelemetry-instrumentation-logging` (log correlation — not tested here)

## How to run

```bash
cd python-test
uv sync
./generate_protos.sh
uv run pytest -v
```

The test verifies:
- gRPC server starts and handles requests
- Client successfully calls server
- OTel produces CLIENT and SERVER spans with matching trace IDs
- Parent-child span relationship is correct
