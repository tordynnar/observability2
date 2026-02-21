// Package telemetry initializes OpenTelemetry tracing, logging, and slog integration.
package telemetry

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"os"
	"time"

	"go.opentelemetry.io/contrib/bridges/otelslog"
	"go.opentelemetry.io/contrib/exporters/autoexport"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/propagation"
	sdklog "go.opentelemetry.io/otel/sdk/log"
	"go.opentelemetry.io/otel/sdk/resource"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
	semconv "go.opentelemetry.io/otel/semconv/v1.26.0"
	"go.opentelemetry.io/otel/trace"
)

// Init sets up OpenTelemetry providers and slog. It returns a shutdown function
// that flushes and stops all providers. The caller must invoke shutdown before
// the process exits.
func Init(ctx context.Context) (shutdown func(context.Context) error, err error) {
	serviceName := os.Getenv("OTEL_SERVICE_NAME")
	if serviceName == "" {
		serviceName = "unknown-service"
	}

	res, err := resource.New(ctx,
		resource.WithAttributes(semconv.ServiceName(serviceName)),
	)
	if err != nil {
		return nil, fmt.Errorf("creating resource: %w", err)
	}

	// --- Trace provider ---
	//
	// autoexport.NewSpanExporter reads OTEL_TRACES_EXPORTER:
	//   "console" → stdouttrace (JSON to stdout)
	//   "otlp"    → OTLP exporter (reads OTEL_EXPORTER_OTLP_* vars)
	//   "none"    → no-op
	// Defaults to OTLP when unset; our shell scripts set it to "console".

	traceExp, err := autoexport.NewSpanExporter(ctx)
	if err != nil {
		return nil, fmt.Errorf("creating trace exporter: %w", err)
	}

	tp := sdktrace.NewTracerProvider(
		sdktrace.WithResource(res),
		sdktrace.WithBatcher(traceExp, sdktrace.WithBatchTimeout(time.Millisecond)),
	)
	otel.SetTracerProvider(tp)

	// --- Log provider ---
	//
	// autoexport.NewLogExporter reads OTEL_LOGS_EXPORTER:
	//   "console" → stdoutlog (JSON to stdout)
	//   "otlp"    → OTLP exporter
	//   "none"    → no-op
	// Defaults to OTLP when unset; our shell scripts set it to "console".

	logExp, err := autoexport.NewLogExporter(ctx)
	if err != nil {
		return nil, fmt.Errorf("creating log exporter: %w", err)
	}

	lp := sdklog.NewLoggerProvider(
		sdklog.WithResource(res),
		sdklog.WithProcessor(sdklog.NewBatchProcessor(logExp, sdklog.WithExportTimeout(time.Millisecond))),
	)

	// --- Propagator ---

	otel.SetTextMapPropagator(propagation.NewCompositeTextMapPropagator(
		propagation.TraceContext{},
		propagation.Baggage{},
	))

	// --- slog setup ---
	//
	// Fan-out handler: each log record goes to both:
	//   1. stderr (text, with trace_id/span_id injected)
	//   2. OTel Logs pipeline (bridged via otelslog)

	stderrHandler := &traceContextHandler{inner: slog.NewTextHandler(os.Stderr, &slog.HandlerOptions{Level: slog.LevelInfo})}
	otelHandler := otelslog.NewHandler("", otelslog.WithLoggerProvider(lp))

	slog.SetDefault(slog.New(&fanOutHandler{handlers: []slog.Handler{stderrHandler, otelHandler}}))

	// --- Shutdown function ---

	shutdown = func(ctx context.Context) error {
		return errors.Join(tp.Shutdown(ctx), lp.Shutdown(ctx))
	}
	return shutdown, nil
}

// traceContextHandler wraps a slog.Handler, adding trace_id and span_id
// attributes extracted from the context.
type traceContextHandler struct {
	inner slog.Handler
}

func (h *traceContextHandler) Enabled(ctx context.Context, level slog.Level) bool {
	return h.inner.Enabled(ctx, level)
}

func (h *traceContextHandler) Handle(ctx context.Context, r slog.Record) error {
	if sc := trace.SpanContextFromContext(ctx); sc.IsValid() {
		r.AddAttrs(
			slog.String("trace_id", sc.TraceID().String()),
			slog.String("span_id", sc.SpanID().String()),
		)
	}
	return h.inner.Handle(ctx, r)
}

func (h *traceContextHandler) WithAttrs(attrs []slog.Attr) slog.Handler {
	return &traceContextHandler{inner: h.inner.WithAttrs(attrs)}
}

func (h *traceContextHandler) WithGroup(name string) slog.Handler {
	return &traceContextHandler{inner: h.inner.WithGroup(name)}
}

// fanOutHandler delegates each log record to multiple slog handlers.
type fanOutHandler struct {
	handlers []slog.Handler
}

func (h *fanOutHandler) Enabled(ctx context.Context, level slog.Level) bool {
	for _, handler := range h.handlers {
		if handler.Enabled(ctx, level) {
			return true
		}
	}
	return false
}

func (h *fanOutHandler) Handle(ctx context.Context, r slog.Record) error {
	var errs []error
	for _, handler := range h.handlers {
		// Clone the record so handlers don't interfere with each other.
		errs = append(errs, handler.Handle(ctx, r.Clone()))
	}
	return errors.Join(errs...)
}

func (h *fanOutHandler) WithAttrs(attrs []slog.Attr) slog.Handler {
	handlers := make([]slog.Handler, len(h.handlers))
	for i, handler := range h.handlers {
		handlers[i] = handler.WithAttrs(attrs)
	}
	return &fanOutHandler{handlers: handlers}
}

func (h *fanOutHandler) WithGroup(name string) slog.Handler {
	handlers := make([]slog.Handler, len(h.handlers))
	for i, handler := range h.handlers {
		handlers[i] = handler.WithGroup(name)
	}
	return &fanOutHandler{handlers: handlers}
}
