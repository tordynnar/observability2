use opentelemetry::global;
use opentelemetry::propagation::{Extractor, Injector};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use tonic::metadata::{KeyRef, MetadataMap};
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Guard that flushes and shuts down OTel providers when dropped or explicitly shut down.
pub struct TelemetryGuard {
    tracer_provider: SdkTracerProvider,
    logger_provider: SdkLoggerProvider,
}

impl TelemetryGuard {
    /// Flush all pending spans and log records, then shut down providers.
    pub fn shutdown(self) {
        if let Err(e) = self.tracer_provider.shutdown() {
            eprintln!("Error shutting down tracer provider: {e}");
        }
        if let Err(e) = self.logger_provider.shutdown() {
            eprintln!("Error shutting down logger provider: {e}");
        }
    }
}

/// Initialize OpenTelemetry tracing, logging, and the `tracing` subscriber.
///
/// Sets up:
/// - A `SdkTracerProvider` with a stdout `SpanExporter` (batch delay via `OTEL_BSP_SCHEDULE_DELAY`)
/// - A `SdkLoggerProvider` with a stdout `LogExporter` (batch delay via `OTEL_BLRP_SCHEDULE_DELAY`)
/// - W3C TraceContext propagator
/// - A layered `tracing` subscriber:
///   - `fmt::Layer` for human-readable logs on stderr
///   - `OpenTelemetryLayer` bridging tracing spans → OTel spans
///   - `OpenTelemetryTracingBridge` bridging tracing events → OTel log records
pub fn init() -> TelemetryGuard {
    // Resource — reads OTEL_SERVICE_NAME and OTEL_RESOURCE_ATTRIBUTES from the environment
    // via the built-in SdkProvidedResourceDetector and EnvResourceDetector.
    let resource = Resource::builder().build();

    // Trace provider — stdout exporter with batch processor.
    // Batch delay is controlled by OTEL_BSP_SCHEDULE_DELAY env var (set to 1ms in run scripts).
    let span_exporter = opentelemetry_stdout::SpanExporter::default();
    let tracer_provider = SdkTracerProvider::builder()
        .with_batch_exporter(span_exporter)
        .with_resource(resource.clone())
        .build();

    // Get a tracer for the tracing-opentelemetry layer before setting the global provider.
    let tracer = tracer_provider.tracer("app");
    global::set_tracer_provider(tracer_provider.clone());

    // Log provider — stdout exporter with batch processor.
    // Batch delay is controlled by OTEL_BLRP_SCHEDULE_DELAY env var (set to 1ms in run scripts).
    let log_exporter = opentelemetry_stdout::LogExporter::default();
    let logger_provider = SdkLoggerProvider::builder()
        .with_batch_exporter(log_exporter)
        .with_resource(resource)
        .build();

    // W3C TraceContext propagator.
    global::set_text_map_propagator(TraceContextPropagator::new());

    // Layered tracing subscriber:
    // 1. fmt layer — human-readable output on stderr
    // 2. OpenTelemetryLayer — bridges tracing spans to OTel spans
    // 3. OpenTelemetryTracingBridge — bridges tracing events to OTel log records
    let fmt_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);
    let otel_trace_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    let otel_log_layer =
        opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(&logger_provider);

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(fmt_layer)
        .with(otel_trace_layer)
        .with(otel_log_layer)
        .init();

    TelemetryGuard {
        tracer_provider,
        logger_provider,
    }
}

// --- Context propagation helpers ---

/// Wraps a `&mut MetadataMap` so the OTel propagator can inject trace context into gRPC metadata.
struct MetadataInjector<'a>(&'a mut MetadataMap);

impl Injector for MetadataInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        if let Ok(key) = tonic::metadata::MetadataKey::from_bytes(key.as_bytes()) {
            if let Ok(val) = value.parse() {
                self.0.insert(key, val);
            }
        }
    }
}

/// Wraps a `&MetadataMap` so the OTel propagator can extract trace context from gRPC metadata.
struct MetadataExtractor<'a>(&'a MetadataMap);

impl Extractor for MetadataExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|v| v.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0
            .keys()
            .filter_map(|k| match k {
                KeyRef::Ascii(k) => Some(k.as_str()),
                _ => None,
            })
            .collect()
    }
}

/// Inject the current span's OTel context into gRPC metadata (for outgoing requests).
pub fn inject_trace_context(metadata: &mut MetadataMap) {
    let cx = Span::current().context();
    global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&cx, &mut MetadataInjector(metadata));
    });
}

/// Extract OTel context from gRPC metadata (for incoming requests).
pub fn extract_trace_context(metadata: &MetadataMap) -> opentelemetry::Context {
    global::get_text_map_propagator(|propagator| propagator.extract(&MetadataExtractor(metadata)))
}
