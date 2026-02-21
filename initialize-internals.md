# What `initialize()` Does — and What Breaks Without `opentelemetry-distro`

This document traces every line of code that executes when you call `initialize()` from `opentelemetry.instrumentation.auto_instrumentation`, and explains exactly what happens if the `opentelemetry-distro` package is not installed.

## The Call

```python
from opentelemetry.instrumentation.auto_instrumentation import initialize
initialize()
```

Source: `.venv/lib/python3.13/site-packages/opentelemetry/instrumentation/auto_instrumentation/__init__.py`, lines 126-170.

## Step-by-Step Execution

### Step 1: PYTHONPATH cleanup (lines 132-136)

```python
if "PYTHONPATH" in environ:
    environ["PYTHONPATH"] = _python_path_without_directory(
        environ["PYTHONPATH"], dirname(abspath(__file__)), pathsep
    )
```

Removes the auto-instrumentation package's own directory from `PYTHONPATH`. This prevents child processes (spawned via `subprocess`, `execl`, etc.) from inheriting auto-instrumentation through `sitecustomize.py`. Irrelevant to our case since we call `initialize()` directly rather than using the `opentelemetry-instrument` CLI launcher.

### Step 2: Optional gevent monkey-patching (lines 140-160)

Checks `OTEL_PYTHON_AUTO_INSTRUMENTATION_EXPERIMENTAL_GEVENT_PATCH`. We don't set this, so the entire block is skipped.

### Step 3: The core sequence (lines 162-166)

```python
try:
    distro = _load_distro()
    distro.configure()
    _load_configurators()
    _load_instrumentors(distro)
except Exception as exc:
    _logger.exception("Failed to auto initialize OpenTelemetry")
    if not swallow_exceptions:
        raise exc
```

Four operations, executed in order. If any raises an exception, it is **logged and swallowed** (because `swallow_exceptions` defaults to `True`). This means `initialize()` never crashes your application — it silently degrades.

---

## `_load_distro()` — Loading the Distribution

Source: `.venv/lib/python3.13/site-packages/opentelemetry/instrumentation/auto_instrumentation/_load.py`, lines 62-84.

```python
def _load_distro() -> BaseDistro:
    distro_name = environ.get(OTEL_PYTHON_DISTRO, None)
    for entry_point in entry_points(group="opentelemetry_distro"):
        try:
            if distro_name is None or distro_name == entry_point.name:
                distro = entry_point.load()()
                if not isinstance(distro, BaseDistro):
                    _logger.debug(
                        "%s is not an OpenTelemetry Distro. Skipping",
                        entry_point.name,
                    )
                    continue
                _logger.debug(
                    "Distribution %s will be configured", entry_point.name
                )
                return distro
        except Exception as exc:
            _logger.exception(
                "Distribution %s configuration failed", entry_point.name
            )
            raise exc
    return DefaultDistro()
```

This function scans Python package entry points in the `opentelemetry_distro` group.

### With `opentelemetry-distro` installed

The package registers this entry point (from `opentelemetry_distro-0.60b1.dist-info/entry_points.txt`):

```ini
[opentelemetry_distro]
distro = opentelemetry.distro:OpenTelemetryDistro
```

So `entry_point.load()()` loads and instantiates `OpenTelemetryDistro`. It passes the `isinstance(distro, BaseDistro)` check and is returned.

### Without `opentelemetry-distro` installed

`entry_points(group="opentelemetry_distro")` returns an empty iterator. The for-loop body never executes. The function falls through to the last line and returns `DefaultDistro()`.

`DefaultDistro` is defined in `opentelemetry/instrumentation/distro.py` (part of the `opentelemetry-instrumentation` package, which is always installed):

```python
class DefaultDistro(BaseDistro):
    def _configure(self, **kwargs):
        pass
```

It does absolutely nothing.

---

## `distro.configure()` — Configuring the Distribution

Back in `initialize()`, the next call is `distro.configure()`, which calls `self._configure()`.

### With `opentelemetry-distro` installed

`OpenTelemetryDistro._configure()` runs (from `opentelemetry/distro/__init__.py`):

```python
def _configure(self, **kwargs):
    os.environ.setdefault(OTEL_TRACES_EXPORTER, "otlp")
    os.environ.setdefault(OTEL_METRICS_EXPORTER, "otlp")
    os.environ.setdefault(OTEL_LOGS_EXPORTER, "otlp")
    os.environ.setdefault(OTEL_EXPORTER_OTLP_PROTOCOL, "grpc")
```

This sets **default** exporter values to OTLP/gRPC, but only if the env vars are not already set. Since our `run_server.sh` explicitly sets `OTEL_TRACES_EXPORTER=console`, `OTEL_LOGS_EXPORTER=console`, and `OTEL_METRICS_EXPORTER=none`, the `setdefault` calls are all no-ops in our case. The distro's configure step effectively does nothing for us.

### Without `opentelemetry-distro` installed

`DefaultDistro._configure()` is a `pass` — does nothing. In our case this is functionally identical to the installed case, because we set all the env vars explicitly.

**Conclusion: The distro step is irrelevant for us either way.** It only matters if you want OTLP defaults without setting env vars yourself.

---

## `_load_configurators()` — Setting Up the SDK

Source: `_load.py`, lines 160-189.

```python
def _load_configurators():
    configurator_name = environ.get(OTEL_PYTHON_CONFIGURATOR, None)
    configured = None
    for entry_point in entry_points(group="opentelemetry_configurator"):
        if configured is not None:
            _logger.warning(
                "Configuration of %s not loaded, %s already loaded",
                entry_point.name,
                configured,
            )
            continue
        try:
            if (
                configurator_name is None
                or configurator_name == entry_point.name
            ):
                entry_point.load()().configure(
                    auto_instrumentation_version=__version__
                )
                configured = entry_point.name
            else:
                _logger.warning(
                    "Configuration of %s not loaded because %s is set by %s",
                    entry_point.name,
                    configurator_name,
                    OTEL_PYTHON_CONFIGURATOR,
                )
        except Exception as exc:
            _logger.exception("Configuration of %s failed", entry_point.name)
            raise exc
```

This is where the critical difference is.

### With `opentelemetry-distro` installed

The package registers this entry point:

```ini
[opentelemetry_configurator]
configurator = opentelemetry.distro:OpenTelemetryConfigurator
```

`OpenTelemetryConfigurator` (from `opentelemetry/distro/__init__.py`) is:

```python
class OpenTelemetryConfigurator(_OTelSDKConfigurator):
    pass
```

It inherits everything from `_OTelSDKConfigurator` (from `opentelemetry/sdk/_configuration/__init__.py`):

```python
class _OTelSDKConfigurator(_BaseConfigurator):
    def _configure(self, **kwargs):
        _initialize_components(**kwargs)
```

So `entry_point.load()().configure(auto_instrumentation_version=__version__)` calls `_initialize_components(auto_instrumentation_version="0.60b1")`.

`_initialize_components()` does the heavy lifting (lines 422-485 of `_configuration/__init__.py`):

1. **Reads exporter env vars** — parses `OTEL_TRACES_EXPORTER`, `OTEL_METRICS_EXPORTER`, `OTEL_LOGS_EXPORTER` to get exporter names (e.g., `"console"`)

2. **Imports exporter classes** — looks up entry points in `opentelemetry_traces_exporter`, `opentelemetry_metrics_exporter`, `opentelemetry_logs_exporter` groups to find `ConsoleSpanExporter`, `ConsoleLogExporter`, etc.

3. **Imports the sampler** — reads `OTEL_TRACES_SAMPLER` (we don't set it, so `None` → default `parentbased_always_on`)

4. **Imports the ID generator** — reads `OTEL_PYTHON_ID_GENERATOR` (we don't set it, so `"random"` → `RandomIdGenerator`)

5. **Creates the Resource** — `Resource.create()` reads `OTEL_SERVICE_NAME` and `OTEL_RESOURCE_ATTRIBUTES`, stamps `telemetry.auto.version`

6. **`_init_tracing()`** — creates a `TracerProvider` with the sampler, ID generator, and resource, then calls `set_tracer_provider(provider)` to register it globally. Wraps each exporter in a `BatchSpanProcessor` and adds it to the provider.

7. **`_init_metrics()`** — creates a `MeterProvider`, wraps exporters in `PeriodicExportingMetricReader`, calls `set_meter_provider(provider)`.

8. **`_init_logging()`** — creates a `LoggerProvider`, wraps exporters in `BatchLogRecordProcessor`, calls `set_logger_provider(provider)`. If `OTEL_PYTHON_LOGGING_AUTO_INSTRUMENTATION_ENABLED=true`, also adds a `LoggingHandler` to Python's root logger and wraps `logging.basicConfig`/`logging.config.fileConfig`/`logging.config.dictConfig` to protect the handler from being removed.

### Without `opentelemetry-distro` installed

`entry_points(group="opentelemetry_configurator")` returns an empty iterator. The for-loop never executes. **`_load_configurators()` returns silently without doing anything.**

This means:

- **No `TracerProvider` is created.** The global tracer provider remains a `ProxyTracerProvider`.
- **No `MeterProvider` is created.** The global meter provider remains a proxy/no-op.
- **No `LoggerProvider` is created.** No OTel log records are emitted.
- **No `BatchSpanProcessor` is created.** There is no span pipeline.
- **No `BatchLogRecordProcessor` is created.** There is no log pipeline.
- **No exporters are instantiated.** `ConsoleSpanExporter`, `ConsoleLogExporter` are never created.
- **No `LoggingHandler` is attached** to the root logger (the `OTEL_PYTHON_LOGGING_AUTO_INSTRUMENTATION_ENABLED` env var is never read).
- **No `Resource` is created.** `OTEL_SERVICE_NAME` is never read by the SDK.

---

## `_load_instrumentors(distro)` — Applying Instrumentation

Source: `_load.py`, lines 87-157.

This function iterates over `entry_points(group="opentelemetry_instrumentor")`. The gRPC and logging instrumentation packages register these entry points (they are installed independently of `opentelemetry-distro`):

```
opentelemetry_instrumentor:
  grpc_server = opentelemetry.instrumentation.grpc:GrpcInstrumentorServer
  grpc_client = opentelemetry.instrumentation.grpc:GrpcInstrumentorClient
  logging = opentelemetry.instrumentation.logging:LoggingInstrumentor
```

For each entry point, `_load_instrumentors` calls `distro.load_instrumentor(entry_point, skip_dep_check=True)`, which in `BaseDistro` does:

```python
def load_instrumentor(self, entry_point: EntryPoint, **kwargs):
    instrumentor: BaseInstrumentor = entry_point.load()
    instrumentor().instrument(**kwargs)
```

### With or without `opentelemetry-distro`

This step is **identical** in both cases. `DefaultDistro` inherits `load_instrumentor` from `BaseDistro` without overriding it. The instrumentors are loaded and `.instrument()` is called on each:

- **`GrpcInstrumentorServer.instrument()`** — monkey-patches `grpc.aio.server()` to add `OpenTelemetryServerInterceptor`
- **`GrpcInstrumentorClient.instrument()`** — monkey-patches `grpc.aio.insecure_channel()` (and other channel constructors) to add `OpenTelemetryClientInterceptor`
- **`LoggingInstrumentor.instrument()`** — patches `logging.Logger.makeRecord` to inject `otelTraceID`, `otelSpanID`, `otelServiceName`, `otelTraceSampled` into every `LogRecord`, and patches `logging.basicConfig` to use a format string that includes these fields

**The monkey-patching happens regardless of whether a TracerProvider is configured.** The interceptors and logging patches are installed into the running process.

---

## What Happens at Request Time Without `opentelemetry-distro`

### The gRPC interceptors try to create spans

When a gRPC request comes in, the server interceptor calls something like:

```python
tracer = trace.get_tracer(__name__)
with tracer.start_as_current_span(...) as span:
    # handle request
```

`trace.get_tracer()` goes through the global `ProxyTracerProvider`, which checks the `_TRACER_PROVIDER` module-level variable (from `opentelemetry/trace/__init__.py`):

```python
class ProxyTracerProvider(TracerProvider):
    def get_tracer(self, ...):
        if _TRACER_PROVIDER:
            return _TRACER_PROVIDER.get_tracer(...)
        return ProxyTracer(...)
```

Since no configurator ran, `set_tracer_provider()` was never called, so `_TRACER_PROVIDER` is `None`. A `ProxyTracer` is returned. `ProxyTracer._tracer` delegates to `self._noop_tracer` (a `NoOpTracer`):

```python
class ProxyTracer(Tracer):
    @property
    def _tracer(self) -> Tracer:
        if self._real_tracer:
            return self._real_tracer
        if _TRACER_PROVIDER:
            self._real_tracer = _TRACER_PROVIDER.get_tracer(...)
            return self._real_tracer
        return self._noop_tracer  # <-- this path
```

`NoOpTracer.start_span()` returns a `NonRecordingSpan` with an `INVALID` span context (trace_id=0, span_id=0). The span is never recorded, never exported.

### The logging instrumentation injects zero trace context

`LoggingInstrumentor` patches `Logger.makeRecord` to read the current span's context:

```python
otelTraceID = span.get_span_context().trace_id
otelSpanID = span.get_span_context().span_id
```

Since the active span is a `NonRecordingSpan` with an INVALID context, `trace_id` and `span_id` are both `0`. Every log line shows `[trace_id=0 span_id=0 ...]`.

### Context propagation is broken

The client interceptor would call `inject(carrier=metadata)` to write a `traceparent` header. But the active span has trace_id=0, so either:
- No `traceparent` is injected (the propagator skips invalid contexts), or
- A `traceparent` with `00000000000000000000000000000000` is injected, which the server ignores as invalid

Either way, the server gets no parent context. Even if it could create spans, they would be unlinked roots.

### No OTel log records are exported

Without a `LoggerProvider`, the `LoggingHandler` is never attached to the root logger. Python log records are emitted normally (to stderr via the `StreamHandler` from `basicConfig`), but they are never bridged to the OTel Logs pipeline. No structured JSON log records appear on stdout.

---

## Summary: With vs. Without `opentelemetry-distro`

| Aspect | With `opentelemetry-distro` | Without |
|---|---|---|
| `_load_distro()` returns | `OpenTelemetryDistro` | `DefaultDistro` |
| `distro.configure()` | Sets OTLP defaults (no-op if env vars are set) | No-op |
| `_load_configurators()` | Runs `OpenTelemetryConfigurator` → `_initialize_components()` | **Does nothing** (no entry points found) |
| `TracerProvider` | Real SDK `TracerProvider` with `BatchSpanProcessor` + exporter | **None** — `ProxyTracerProvider` delegates to `NoOpTracer` |
| `LoggerProvider` | Real SDK `LoggerProvider` with `BatchLogRecordProcessor` + exporter | **None** — no OTel log pipeline |
| `MeterProvider` | Real SDK `MeterProvider` | **None** — no metrics |
| Monkey-patching (gRPC, logging) | Applied | **Applied** (identical) |
| Spans created | Real spans, recorded and exported | `NonRecordingSpan` (trace_id=0, span_id=0) — **silently discarded** |
| Log/trace correlation | `trace_id` and `span_id` populated from active span | Always `0` — **no correlation** |
| W3C propagation | `traceparent` injected/extracted with real trace IDs | Invalid/absent — **no distributed tracing** |
| OTel log records (JSON) | Exported via `ConsoleLogExporter` | **Not produced** |
| Python log output (stderr) | Format includes `[trace_id=... span_id=...]` fields | Format includes fields, but values are always `0` |
| Application behavior | Unchanged (instrumentation is transparent) | Unchanged (instrumentation is transparent) |

## The Key Insight

`opentelemetry-distro` provides the **configurator**, not the instrumentors. Instrumentors are installed and activated regardless. But without a configurator, there is no SDK — no providers, no processors, no exporters. The instrumentors faithfully create no-op spans and inject zero-valued trace context into every log line. Your application runs fine, but you get no telemetry.

The package split is intentional: it lets you install instrumentors in development/CI without any SDK overhead, then add `opentelemetry-distro` (or a vendor-specific distro) in production to actually collect telemetry. But it's easy to forget the distro package and end up with an application that *looks* instrumented (the log format has trace fields) but produces nothing useful (all zeroes).
