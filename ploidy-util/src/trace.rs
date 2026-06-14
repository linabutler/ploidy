//! Tracing support for generated clients.

use http::HeaderMap;
use opentelemetry::global::get_text_map_propagator;
use opentelemetry_http::HeaderInjector;
use reqwest::RequestBuilder;
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;

/// Adds trace context request headers, if there is one and
/// a global [`TextMapPropagator`] is [set].
///
/// [`TextMapPropagator`]: opentelemetry::propagation::TextMapPropagator
/// [set]: opentelemetry::global::set_text_map_propagator
pub fn propagate(span: Span, request: RequestBuilder) -> RequestBuilder {
    let context = span.context();
    let mut headers = HeaderMap::new();
    get_text_map_propagator(|p| {
        p.inject_context(&context, &mut HeaderInjector(&mut headers));
    });
    // We intentionally use `request.headers()` to replace any
    // existing trace headers.
    request.headers(headers)
}
