# OpenTelemetry + gRPC in Go

## Table of Contents

1. [Dependencies](#dependencies)
2. [How It Works](#how-it-works)
3. [Telemetry Initialization](#telemetry-initialization)
4. [Server Code](#server-code)
5. [Client Code](#client-code)
6. [Launch Scripts](#launch-scripts)
7. [Environment Variables](#environment-variables)
8. [Design Decisions](#design-decisions)
9. [Testing That It Works](#testing-that-it-works)
10. [Troubleshooting](#troubleshooting)

---

## Dependencies

Go 1.21+ (for `log/slog`). Dependencies managed by Go modules (`go.mod`).

### Direct Dependencies

```
go.opentelemetry.io/contrib/bridges/otelslog         v0.15.0
go.opentelemetry.io/contrib/exporters/autoexport      v0.65.0
go.opentelemetry.io/contrib/instrumentation/google.golang.org/grpc/otelgrpc  v0.65.0
go.opentelemetry.io/contrib/propagators/autoprop      v0.65.0
go.opentelemetry.io/otel                              v1.40.0
go.opentelemetry.io/otel/sdk                          v1.40.0
go.opentelemetry.io/otel/sdk/log                      v0.16.0
google.golang.org/grpc                                v1.79.1
google.golang.org/protobuf                            v1.36.11
```

### What each package does

| Package | Role |
|---------|------|
| `google.golang.org/grpc` | gRPC runtime: `grpc.NewServer()`, `grpc.NewClient()`, stats handlers, transport. |
| `google.golang.org/protobuf` | Protocol Buffers runtime. Generated `.pb.go` files depend on it. |
| `go.opentelemetry.io/otel` | Public OTel API: `otel.SetTracerProvider()`, `otel.SetTextMapPropagator()`. No-op until an SDK is registered. |
| `go.opentelemetry.io/otel/sdk` | Concrete SDK: `TracerProvider`, `BatchSpanProcessor`. Records and exports spans. |
| `go.opentelemetry.io/otel/sdk/log` | Logs SDK: `LoggerProvider`, `BatchProcessor`. Records and exports OTel log records. |
| `go.opentelemetry.io/contrib/exporters/autoexport` | Reads `OTEL_TRACES_EXPORTER` and `OTEL_LOGS_EXPORTER` env vars, returns the matching exporter. Supports `console`, `otlp`, and `none`. Pulls in OTLP exporters as transitive deps -- no extra packages needed to switch. |
| `go.opentelemetry.io/contrib/propagators/autoprop` | Reads `OTEL_PROPAGATORS` env var, returns the matching propagator. Supports `tracecontext`, `baggage`, `b3`, `jaeger`, `xray`, `ottrace`, `none`. |
| `go.opentelemetry.io/contrib/instrumentation/.../otelgrpc` | gRPC stats handlers (`NewServerHandler()`, `NewClientHandler()`) that auto-create spans for every RPC. Server handler extracts trace context; client handler injects it. |
| `go.opentelemetry.io/contrib/bridges/otelslog` | Bridges `log/slog` into OTel Logs pipeline. Each `slog` record becomes an OTel `LogRecord` with trace context. |

---

## How It Works

Go uses **explicit initialization** -- there is no auto-discovery, no monkey-patching, no magic. Every piece of instrumentation is wired explicitly in code:

1. `telemetry.Init()` creates providers, exporters, propagators, and bridges slog
2. Stats handlers are passed to `grpc.NewServer()` and `grpc.NewClient()` as options
3. `slog.InfoContext(ctx, ...)` passes context explicitly for log correlation

This is the Go philosophy: explicit is better than implicit. The tradeoff is more wiring code, but nothing is hidden.

### Context passing is explicit

Go has no thread-local context. To correlate logs with traces, every log call must explicitly pass the `context.Context` that carries the span:

```go
// This works -- log record gets trace_id and span_id:
slog.InfoContext(ctx, "Received request", "name", req.GetName())

// This does NOT correlate -- log record has zero trace_id/span_id:
slog.Info("Received request", "name", req.GetName())
```

The `ctx` parameter is what connects the log to the trace. If you forget it, there's no compiler warning -- `slog.Info()` is perfectly valid, it just doesn't correlate.

---

## Telemetry Initialization

```go
package telemetry

import (
	"context"
	"errors"
	"log/slog"
	"os"
	"strconv"
	"time"

	"go.opentelemetry.io/contrib/bridges/otelslog"
	"go.opentelemetry.io/contrib/exporters/autoexport"
	"go.opentelemetry.io/contrib/propagators/autoprop"
	"go.opentelemetry.io/otel"
	sdklog "go.opentelemetry.io/otel/sdk/log"
	"go.opentelemetry.io/otel/sdk/resource"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
)

func Init(ctx context.Context) (shutdown func(context.Context) error, err error) {
	res, err := resource.New(ctx, resource.WithFromEnv())
	if err != nil {
		return nil, err
	}

	traceExp, err := autoexport.NewSpanExporter(ctx)
	if err != nil {
		return nil, err
	}
	bspDelay := envDurationMs("OTEL_BSP_SCHEDULE_DELAY", time.Millisecond)
	tp := sdktrace.NewTracerProvider(
		sdktrace.WithResource(res),
		sdktrace.WithBatcher(traceExp, sdktrace.WithBatchTimeout(bspDelay)),
	)
	otel.SetTracerProvider(tp)

	logExp, err := autoexport.NewLogExporter(ctx)
	if err != nil {
		return nil, err
	}
	blrpDelay := envDurationMs("OTEL_BLRP_SCHEDULE_DELAY", time.Millisecond)
	lp := sdklog.NewLoggerProvider(
		sdklog.WithResource(res),
		sdklog.WithProcessor(
			sdklog.NewBatchProcessor(logExp, sdklog.WithExportTimeout(blrpDelay)),
		),
	)

	otel.SetTextMapPropagator(autoprop.NewTextMapPropagator())

	slog.SetDefault(slog.New(otelslog.NewHandler("", otelslog.WithLoggerProvider(lp))))

	shutdown = func(ctx context.Context) error {
		return errors.Join(tp.Shutdown(ctx), lp.Shutdown(ctx))
	}
	return shutdown, nil
}

func envDurationMs(key string, def time.Duration) time.Duration {
	v := os.Getenv(key)
	if v == "" {
		return def
	}
	ms, err := strconv.Atoi(v)
	if err != nil {
		return def
	}
	return time.Duration(ms) * time.Millisecond
}
```

### What `Init()` does step by step

1. **Creates a Resource** via `resource.WithFromEnv()` -- reads `OTEL_SERVICE_NAME` and `OTEL_RESOURCE_ATTRIBUTES` from the environment.
2. **Creates a trace exporter** via `autoexport.NewSpanExporter(ctx)` -- reads `OTEL_TRACES_EXPORTER` to select `console`, `otlp`, or `none`.
3. **Creates a `TracerProvider`** with a `BatchSpanProcessor` wrapping the trace exporter. Batch delay is read from `OTEL_BSP_SCHEDULE_DELAY`.
4. **Creates a log exporter** via `autoexport.NewLogExporter(ctx)` -- reads `OTEL_LOGS_EXPORTER`.
5. **Creates a `LoggerProvider`** with a `BatchProcessor` wrapping the log exporter. Batch delay is read from `OTEL_BLRP_SCHEDULE_DELAY`.
6. **Sets the global propagator** via `autoprop.NewTextMapPropagator()` -- reads `OTEL_PROPAGATORS`.
7. **Bridges slog** -- replaces the default `slog` handler with `otelslog.NewHandler`, which converts every `slog` record into an OTel `LogRecord` with trace context.
8. **Returns a shutdown function** that flushes both providers. Uses `errors.Join` to combine any errors.

---

## Server Code

```go
package main

import (
	"context"
	"log/slog"
	"net"
	"os"
	"os/signal"

	"google.golang.org/grpc"
	"go.opentelemetry.io/contrib/instrumentation/google.golang.org/grpc/otelgrpc"

	pb "observability2/go/pb"
	"observability2/go/internal/telemetry"
)

type greeterServer struct {
	pb.UnimplementedGreeterServer
}

func (s *greeterServer) SayHello(ctx context.Context, req *pb.HelloRequest) (*pb.HelloReply, error) {
	slog.InfoContext(ctx, "Received request", "name", req.GetName())
	return &pb.HelloReply{Message: "Hello, " + req.GetName() + "!"}, nil
}

func main() {
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	defer stop()

	shutdown, err := telemetry.Init(ctx)
	if err != nil {
		slog.Error("failed to initialize telemetry", "error", err)
		os.Exit(1)
	}
	defer func() {
		if err := shutdown(context.Background()); err != nil {
			slog.Error("telemetry shutdown error", "error", err)
		}
	}()

	lis, err := net.Listen("tcp", "[::]:50051")
	if err != nil {
		slog.Error("failed to listen", "error", err)
		os.Exit(1)
	}

	srv := grpc.NewServer(grpc.StatsHandler(otelgrpc.NewServerHandler()))
	pb.RegisterGreeterServer(srv, &greeterServer{})

	slog.Info("Server starting on port 50051")

	go func() {
		<-ctx.Done()
		slog.Info("Shutting down server")
		srv.GracefulStop()
	}()

	if err := srv.Serve(lis); err != nil {
		slog.Error("server error", "error", err)
		os.Exit(1)
	}
}
```

Key points:
- **`slog.InfoContext(ctx, ...)`** -- always use the `*Context` variants to correlate logs with traces. The `ctx` carries the active span created by the stats handler.
- **`grpc.StatsHandler(otelgrpc.NewServerHandler())`** -- this is the recommended (non-deprecated) approach. The stats handler automatically extracts `traceparent` from incoming metadata, creates a SERVER span, and injects it into the context.
- **Signal handling** -- `signal.NotifyContext` creates a context cancelled on SIGINT. The goroutine watching `ctx.Done()` calls `GracefulStop()`.
- **Shutdown uses `context.Background()`** -- the deferred shutdown creates a fresh context (not the cancelled signal context) to ensure the flush can complete.

---

## Client Code

```go
package main

import (
	"context"
	"log/slog"
	"os"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
	"go.opentelemetry.io/contrib/instrumentation/google.golang.org/grpc/otelgrpc"

	pb "observability2/go/pb"
	"observability2/go/internal/telemetry"
)

func main() {
	ctx := context.Background()

	shutdown, err := telemetry.Init(ctx)
	if err != nil {
		slog.Error("failed to initialize telemetry", "error", err)
		os.Exit(1)
	}
	defer func() {
		if err := shutdown(context.Background()); err != nil {
			slog.Error("telemetry shutdown error", "error", err)
		}
	}()

	conn, err := grpc.NewClient("localhost:50051",
		grpc.WithTransportCredentials(insecure.NewCredentials()),
		grpc.WithStatsHandler(otelgrpc.NewClientHandler()),
	)
	if err != nil {
		slog.Error("failed to connect", "error", err)
		os.Exit(1)
	}
	defer conn.Close()

	client := pb.NewGreeterClient(conn)

	ctx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()

	resp, err := client.SayHello(ctx, &pb.HelloRequest{Name: "World"})
	if err != nil {
		slog.Error("SayHello failed", "error", err)
		os.Exit(1)
	}

	slog.InfoContext(ctx, "Greeter response", "message", resp.GetMessage())
}
```

Key points:
- **`grpc.NewClient`** (not the deprecated `grpc.Dial`) -- creates a client connection with the OTel stats handler.
- **`otelgrpc.NewClientHandler()`** -- automatically creates CLIENT spans and injects `traceparent` into outgoing metadata.
- **5-second timeout** -- prevents the RPC from hanging indefinitely.
- **`slog.InfoContext(ctx, ...)`** -- the `ctx` carries the client span, so the log record gets the client's trace context.

---

## Launch Scripts

### run_server.sh

```bash
#!/usr/bin/env bash
set -euo pipefail

export OTEL_SERVICE_NAME="grpc-server"
export OTEL_TRACES_EXPORTER="console"
export OTEL_LOGS_EXPORTER="console"
export OTEL_PROPAGATORS="tracecontext,baggage"
export OTEL_BSP_SCHEDULE_DELAY="1"
export OTEL_BLRP_SCHEDULE_DELAY="1"

exec go run ./cmd/server
```

### run_client.sh

```bash
#!/usr/bin/env bash
set -euo pipefail

export OTEL_SERVICE_NAME="grpc-client"
export OTEL_TRACES_EXPORTER="console"
export OTEL_LOGS_EXPORTER="console"
export OTEL_PROPAGATORS="tracecontext,baggage"
export OTEL_BSP_SCHEDULE_DELAY="1"
export OTEL_BLRP_SCHEDULE_DELAY="1"

exec go run ./cmd/client
```

---

## Environment Variables

| Variable | Value | Who Reads It |
|----------|-------|-------------|
| `OTEL_SERVICE_NAME` | `grpc-server` / `grpc-client` | `resource.WithFromEnv()` -- sets `service.name` on the resource |
| `OTEL_TRACES_EXPORTER` | `console` | `autoexport.NewSpanExporter()` -- `console`, `otlp`, or `none` |
| `OTEL_LOGS_EXPORTER` | `console` | `autoexport.NewLogExporter()` -- `console`, `otlp`, or `none` |
| `OTEL_PROPAGATORS` | `tracecontext,baggage` | `autoprop.NewTextMapPropagator()` -- also supports `b3`, `jaeger`, `xray` |
| `OTEL_BSP_SCHEDULE_DELAY` | `1` | `telemetry.Init()` via `envDurationMs()` -- span batch flush (ms) |
| `OTEL_BLRP_SCHEDULE_DELAY` | `1` | `telemetry.Init()` via `envDurationMs()` -- log batch flush (ms) |

Note: Go does not need `OTEL_METRICS_EXPORTER=none` because the code doesn't set up a `MeterProvider`. The `otelgrpc` stats handlers can produce metrics, but without a registered `MeterProvider` they go to a no-op.

---

## Design Decisions

### 1. Stats handlers over interceptors

The `otelgrpc` package offers both interceptors (`UnaryServerInterceptor`, `UnaryClientInterceptor`) and stats handlers (`NewServerHandler()`, `NewClientHandler()`). The interceptors are **deprecated**. Stats handlers cover both unary and streaming RPCs with a single registration point and are the recommended approach.

### 2. `log/slog` for logging

Go's stdlib structured logger (since Go 1.21). Native `context.Context` support via `InfoContext`/`WarnContext`/etc. is essential for trace correlation. The `otelslog` bridge connects it directly to the OTel Logs pipeline without any third-party logging framework.

### 3. `autoexport` and `autoprop` for env-var-driven config

These `contrib` packages read standard `OTEL_*` environment variables and return the appropriate exporter/propagator at runtime. `autoexport` pulls in OTLP exporters (gRPC and HTTP) as transitive dependencies, so switching from `console` to `otlp` requires zero code changes and zero new dependencies. `autoprop` supports all standard propagation formats.

### 4. Explicit context passing

Go has no thread-local storage (by design). The `context.Context` must be passed explicitly through the call chain. For log correlation, this means always using `slog.InfoContext(ctx, ...)` instead of `slog.Info(...)`. This is more verbose than Python/Rust but makes the data flow visible and debuggable.

### 5. Proto files committed

Go convention is to commit generated code. This means no `protoc` installation needed to build and run. The proto file includes `option go_package` (which the Python proto doesn't need), so Go maintains its own copy of the proto file.

### 6. Shutdown uses `context.Background()`

The deferred `shutdown(context.Background())` creates a fresh context rather than using the potentially-cancelled signal context. This ensures the flush has a valid, non-cancelled context and can complete successfully. Without this, shutdown might fail silently on Ctrl+C.

### 7. No metrics pipeline

Metrics are deliberately omitted. The `otelgrpc` stats handlers can produce metrics, but without a registered `MeterProvider`, they go to a no-op. This keeps the setup focused on traces + logs. Add a `MeterProvider` later if you need metrics.

---

## Testing That It Works

### Step 1: Start the server and client

```bash
cd go
./run_server.sh    # terminal 1
./run_client.sh    # terminal 2
```

### Step 2: Verify two types of output

**1. OTel spans to stdout (JSON):**
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
        {"Key": "rpc.system.name", "Value": {"Type": "STRING", "Value": "grpc"}}
    ],
    "Resource": [
        {"Key": "service.name", "Value": {"Type": "STRING", "Value": "grpc-server"}}
    ]
}
```

**2. OTel log records to stdout (JSON):**
```json
{
    "Severity": 9,
    "SeverityText": "INFO",
    "Body": {"Type": "String", "Value": "Received request"},
    "Attributes": [{"Key": "name", "Value": {"Type": "String", "Value": "World"}}],
    "TraceID": "5061a996070a831e1a8ef895f8aa4268",
    "SpanID": "fc7fba7097ddcd66"
}
```

### Step 3: Verify trace linkage

1. Find the client span in client stdout -- note its `SpanID`
2. Find the server span in server stdout -- check its `Parent.SpanID`
3. The server's `Parent.SpanID` should equal the client's `SpanID`
4. Both spans share the same `TraceID`

### Step 4: Verify log correlation

The `TraceID` and `SpanID` in the log record should match the server span's `TraceID` and `SpanID`.

---

## Troubleshooting

### 1. Log records have zero trace IDs

**Symptom:** OTel log records show `"TraceID": "00000000000000000000000000000000"`.

**Cause:** `slog.Info()` was used instead of `slog.InfoContext(ctx, ...)`. Without the context, the `otelslog.Handler` cannot find the active span.

**Fix:** Always use the `*Context` variants:
```go
// Wrong:
slog.Info("Received request", "name", req.GetName())

// Right:
slog.InfoContext(ctx, "Received request", "name", req.GetName())
```

### 2. No spans appear at all

**Symptom:** Server runs, client gets a response, but no span JSON appears on stdout.

**Possible causes:**
- Stats handler not attached: check that `grpc.StatsHandler(otelgrpc.NewServerHandler())` is passed to `grpc.NewServer()`.
- `telemetry.Init()` not called or its error not handled: if `Init()` returns an error and you ignore it, no providers are registered.
- `OTEL_TRACES_EXPORTER` not set: if the env var is unset, `autoexport` may default to `otlp`, which fails silently without a collector. Set explicitly to `console`.

**Fix:** Verify all three: `Init()` succeeds, stats handlers are attached, `OTEL_TRACES_EXPORTER=console` is set.

### 3. Client and server spans have different trace IDs

**Symptom:** Both produce spans, but trace IDs don't match.

**Cause:** Propagation is broken. Either:
- Client stats handler not attached: `grpc.WithStatsHandler(otelgrpc.NewClientHandler())` missing from `grpc.NewClient()`.
- `OTEL_PROPAGATORS` not set or wrong value.
- `autoprop.NewTextMapPropagator()` not called in `Init()` or `otel.SetTextMapPropagator()` not called.

**Fix:** Verify the client has its stats handler, and `OTEL_PROPAGATORS=tracecontext,baggage` is set.

### 4. Spans appear only after 5 seconds

**Symptom:** Telemetry is delayed by up to 5 seconds after each request.

**Cause:** `OTEL_BSP_SCHEDULE_DELAY` and `OTEL_BLRP_SCHEDULE_DELAY` are using defaults.

**Fix:** Set both to `1` in launch scripts for development.

### 5. Forgot a new gRPC service's stats handler

**Symptom:** New gRPC service produces no spans, even though the existing one works.

**Cause:** Go has no auto-discovery. Each `grpc.NewServer()` and `grpc.NewClient()` call must explicitly include the stats handler option.

**Fix:** Add `grpc.StatsHandler(otelgrpc.NewServerHandler())` or `grpc.WithStatsHandler(otelgrpc.NewClientHandler())` to every gRPC server/client constructor.

### 6. Shutdown not flushing all spans

**Symptom:** Short-lived programs (like the client) exit without all spans appearing.

**Cause:** The `BatchSpanProcessor` buffers spans. If the process exits before the batch flushes, data is lost.

**Fix:** Ensure `shutdown(context.Background())` is called before exit. Use `defer` in `main()`:
```go
defer func() {
    if err := shutdown(context.Background()); err != nil {
        slog.Error("telemetry shutdown error", "error", err)
    }
}()
```
