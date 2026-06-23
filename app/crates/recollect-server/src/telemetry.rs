//! Telemetry: structured JSON logs always. When `OTEL_EXPORTER_OTLP_ENDPOINT`
//! is set (the docker-compose Grafana LGTM stack speaks OTLP gRPC on :4317) the
//! server exports **all three signals** over OTLP — traces, logs, and metrics.
//! Unset ⇒ JSON logs only; set but no collector listening ⇒ the batch exporters
//! drop their data and the server runs fine either way.
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;

fn resource() -> Resource {
    Resource::builder()
        .with_service_name("recollect-server")
        .build()
}

pub fn init() {
    let env_filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    let fmt = tracing_subscriber::fmt::layer().json();

    match std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        Ok(endpoint) if !endpoint.is_empty() => {
            let res = resource();
            install_metrics(&endpoint, res.clone());
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt)
                .with(trace_layer(&endpoint, res.clone()))
                .with(log_layer(&endpoint, res))
                .init();
        }
        _ => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt)
                .init();
        }
    }
}

/// Span export over OTLP, as a tracing layer.
fn trace_layer<S>(endpoint: &str, resource: Resource) -> impl Layer<S>
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()
        .expect("build OTLP span exporter");
    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();
    let tracer = provider.tracer("recollect-server");
    // Keep the provider alive for the process lifetime and make it global.
    opentelemetry::global::set_tracer_provider(provider);
    tracing_opentelemetry::layer().with_tracer(tracer)
}

/// Log export over OTLP: bridge `tracing` events into OpenTelemetry logs.
fn log_layer<S>(endpoint: &str, resource: Resource) -> impl Layer<S>
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    let exporter = opentelemetry_otlp::LogExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()
        .expect("build OTLP log exporter");
    let provider = SdkLoggerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();
    OpenTelemetryTracingBridge::new(&provider)
}

/// Metric export over OTLP: a periodic reader pushing to the collector. Sets the
/// global meter provider and emits a process-start counter so the metrics
/// pipeline carries a signal even before app metrics are instrumented.
fn install_metrics(endpoint: &str, resource: Resource) {
    let exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()
        .expect("build OTLP metric exporter");
    let reader = PeriodicReader::builder(exporter).build();
    let provider = SdkMeterProvider::builder()
        .with_reader(reader)
        .with_resource(resource)
        .build();
    opentelemetry::global::set_meter_provider(provider);
    opentelemetry::global::meter("recollect-server")
        .u64_counter("recollect.server.starts")
        .build()
        .add(1, &[]);
}
