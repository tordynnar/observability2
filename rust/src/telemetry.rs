use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Guard that shuts down OTel providers on drop.
pub struct TelemetryGuard {
    tracer_provider: SdkTracerProvider,
    logger_provider: SdkLoggerProvider,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Err(e) = self.tracer_provider.shutdown() {
            eprintln!("Error shutting down tracer provider: {e}");
        }
        if let Err(e) = self.logger_provider.shutdown() {
            eprintln!("Error shutting down logger provider: {e}");
        }
    }
}

/// Initialize OpenTelemetry tracing + logging with stdout exporters, and set up
/// the `tracing` subscriber with fmt, OTel trace, and OTel log bridge layers.
pub fn init() -> TelemetryGuard {
    let resource = Resource::builder().build();

    // Trace provider — stdout exporter with batch processor.
    // Batch delay is controlled by OTEL_BSP_SCHEDULE_DELAY (set to 1ms in run scripts).
    let tracer_provider = SdkTracerProvider::builder()
        .with_batch_exporter(opentelemetry_stdout::SpanExporter::default())
        .with_resource(resource.clone())
        .build();
    let tracer = tracer_provider.tracer("app");
    global::set_tracer_provider(tracer_provider.clone());

    // Log provider — stdout exporter with batch processor.
    // Batch delay is controlled by OTEL_BLRP_SCHEDULE_DELAY (set to 1ms in run scripts).
    let logger_provider = SdkLoggerProvider::builder()
        .with_batch_exporter(opentelemetry_stdout::LogExporter::default())
        .with_resource(resource)
        .build();

    global::set_text_map_propagator(TraceContextPropagator::new());

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(
            &logger_provider,
        ))
        .init();

    TelemetryGuard {
        tracer_provider,
        logger_provider,
    }
}
