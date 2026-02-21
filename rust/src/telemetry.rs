use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub struct TelemetryGuard {
    tracer_provider: SdkTracerProvider,
    logger_provider: SdkLoggerProvider,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        let _ = self.tracer_provider.shutdown();
        let _ = self.logger_provider.shutdown();
    }
}

pub fn init() -> TelemetryGuard {
    let resource = Resource::builder().build();

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
        _ => tracer_builder,
    };
    let tracer_provider = tracer_builder.build();
    let tracer = tracer_provider.tracer("app");
    global::set_tracer_provider(tracer_provider.clone());

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
        _ => logger_builder,
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
