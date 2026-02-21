// Package telemetry initializes OpenTelemetry tracing, logging, and slog integration.
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

// Init sets up OpenTelemetry providers and slog. It returns a shutdown function
// that flushes and stops all providers. The caller must invoke shutdown before
// the process exits.
func Init(ctx context.Context) (shutdown func(context.Context) error, err error) {
	// Reads OTEL_SERVICE_NAME and OTEL_RESOURCE_ATTRIBUTES from the environment.
	res, err := resource.New(ctx, resource.WithFromEnv())
	if err != nil {
		return nil, err
	}

	// Trace provider — autoexport reads OTEL_TRACES_EXPORTER ("console", "otlp", "none").
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

	// Log provider — autoexport reads OTEL_LOGS_EXPORTER ("console", "otlp", "none").
	logExp, err := autoexport.NewLogExporter(ctx)
	if err != nil {
		return nil, err
	}
	blrpDelay := envDurationMs("OTEL_BLRP_SCHEDULE_DELAY", time.Millisecond)
	lp := sdklog.NewLoggerProvider(
		sdklog.WithResource(res),
		sdklog.WithProcessor(sdklog.NewBatchProcessor(logExp, sdklog.WithExportTimeout(blrpDelay))),
	)

	// Propagator — reads OTEL_PROPAGATORS, defaults to tracecontext+baggage.
	otel.SetTextMapPropagator(autoprop.NewTextMapPropagator())

	// Bridge slog to the OTel Logs pipeline. Every slog.InfoContext(ctx, ...)
	// call becomes an OTel LogRecord with trace_id/span_id from the context.
	slog.SetDefault(slog.New(otelslog.NewHandler("", otelslog.WithLoggerProvider(lp))))

	shutdown = func(ctx context.Context) error {
		return errors.Join(tp.Shutdown(ctx), lp.Shutdown(ctx))
	}
	return shutdown, nil
}

// envDurationMs reads an environment variable as a millisecond integer and
// returns it as a time.Duration. If the variable is unset or invalid, it
// returns the provided default.
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
