// Package telemetry initializes OpenTelemetry tracing, logging, and slog integration.
package telemetry

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"time"

	"go.opentelemetry.io/contrib/bridges/otelslog"
	"go.opentelemetry.io/contrib/exporters/autoexport"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/propagation"
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
		return nil, fmt.Errorf("creating resource: %w", err)
	}

	// Trace provider — autoexport reads OTEL_TRACES_EXPORTER ("console", "otlp", "none").
	traceExp, err := autoexport.NewSpanExporter(ctx)
	if err != nil {
		return nil, fmt.Errorf("creating trace exporter: %w", err)
	}
	tp := sdktrace.NewTracerProvider(
		sdktrace.WithResource(res),
		sdktrace.WithBatcher(traceExp, sdktrace.WithBatchTimeout(time.Millisecond)),
	)
	otel.SetTracerProvider(tp)

	// Log provider — autoexport reads OTEL_LOGS_EXPORTER ("console", "otlp", "none").
	logExp, err := autoexport.NewLogExporter(ctx)
	if err != nil {
		return nil, fmt.Errorf("creating log exporter: %w", err)
	}
	lp := sdklog.NewLoggerProvider(
		sdklog.WithResource(res),
		sdklog.WithProcessor(sdklog.NewBatchProcessor(logExp, sdklog.WithExportTimeout(time.Millisecond))),
	)

	// Propagator — W3C Trace Context + Baggage.
	otel.SetTextMapPropagator(propagation.NewCompositeTextMapPropagator(
		propagation.TraceContext{},
		propagation.Baggage{},
	))

	// Bridge slog to the OTel Logs pipeline. Every slog.InfoContext(ctx, ...)
	// call becomes an OTel LogRecord with trace_id/span_id from the context.
	slog.SetDefault(slog.New(otelslog.NewHandler("", otelslog.WithLoggerProvider(lp))))

	shutdown = func(ctx context.Context) error {
		return errors.Join(tp.Shutdown(ctx), lp.Shutdown(ctx))
	}
	return shutdown, nil
}
