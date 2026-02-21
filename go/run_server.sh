#!/usr/bin/env bash
set -euo pipefail

export OTEL_SERVICE_NAME="grpc-server"

# Read by autoexport to select the span/log exporters.
# Supported values: "console" (stdout JSON), "otlp", "none".
export OTEL_TRACES_EXPORTER="console"
export OTEL_LOGS_EXPORTER="console"

# Not read by the Go code — set for documentation parity with the Python project.
export OTEL_METRICS_EXPORTER="none"
export OTEL_PROPAGATORS="tracecontext,baggage"

exec go run ./cmd/server
