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

/// Initialize OpenTelemetry tracing + logging, and set up the `tracing`
/// subscriber with fmt, OTel trace, and OTel log bridge layers.
///
/// Exporter selection is driven by environment variables:
/// - `OTEL_TRACES_EXPORTER`: `console`, `otlp`, or `none` (default: none)
/// - `OTEL_LOGS_EXPORTER`: `console`, `otlp`, or `none` (default: none)
pub fn init() -> TelemetryGuard {
    let resource = Resource::builder().build();

    // Trace provider — exporter selected by OTEL_TRACES_EXPORTER.
    // Batch delay is controlled by OTEL_BSP_SCHEDULE_DELAY (set to 1ms in run scripts).
    let traces_exporter = std::env::var("OTEL_TRACES_EXPORTER").unwrap_or_default();
    let mut tracer_builder = SdkTracerProvider::builder().with_resource(resource.clone());
    tracer_builder = match traces_exporter.as_str() {
        "console" => {
            tracer_builder.with_batch_exporter(opentelemetry_stdout::SpanExporter::default())
        }
        "otlp" => tracer_builder.with_batch_exporter(
            opentelemetry_otlp::SpanExporter::builder()
                .with_tonic()
                .build()
                .expect("failed to build OTLP span exporter"),
        ),
        _ => tracer_builder, // "none" or unknown: no exporter
    };
    let tracer_provider = tracer_builder.build();
    let tracer = tracer_provider.tracer("app");
    global::set_tracer_provider(tracer_provider.clone());

    // Log provider — exporter selected by OTEL_LOGS_EXPORTER.
    // Batch delay is controlled by OTEL_BLRP_SCHEDULE_DELAY (set to 1ms in run scripts).
    let logs_exporter = std::env::var("OTEL_LOGS_EXPORTER").unwrap_or_default();
    let mut logger_builder = SdkLoggerProvider::builder().with_resource(resource);
    logger_builder = match logs_exporter.as_str() {
        "console" => {
            logger_builder.with_batch_exporter(opentelemetry_stdout::LogExporter::default())
        }
        "otlp" => logger_builder.with_batch_exporter(
            opentelemetry_otlp::LogExporter::builder()
                .with_tonic()
                .build()
                .expect("failed to build OTLP log exporter"),
        ),
        _ => logger_builder, // "none" or unknown: no exporter
    };
    let logger_provider = logger_builder.build();

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
