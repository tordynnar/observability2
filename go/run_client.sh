#!/usr/bin/env bash
set -euo pipefail

export OTEL_SERVICE_NAME="grpc-client"

# Read by autoexport to select the span/log exporters.
# Supported values: "console" (stdout JSON), "otlp", "none".
export OTEL_TRACES_EXPORTER="console"
export OTEL_LOGS_EXPORTER="console"
export OTEL_PROPAGATORS="tracecontext,baggage"

exec go run ./cmd/client
