#!/usr/bin/env bash
set -euo pipefail

export OTEL_SERVICE_NAME="grpc-client"
export OTEL_TRACES_EXPORTER="console"
export OTEL_LOGS_EXPORTER="console"
export OTEL_METRICS_EXPORTER="none"
export OTEL_PROPAGATORS="tracecontext,baggage"
export OTEL_PYTHON_LOG_CORRELATION="true"
export OTEL_PYTHON_LOG_LEVEL="info"
export OTEL_PYTHON_LOGGING_AUTO_INSTRUMENTATION_ENABLED="true"
export OTEL_BSP_SCHEDULE_DELAY="1"
export OTEL_BLRP_SCHEDULE_DELAY="1"

exec .venv/bin/python client.py
