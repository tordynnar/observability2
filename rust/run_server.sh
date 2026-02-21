#!/usr/bin/env bash
set -euo pipefail

export OTEL_SERVICE_NAME="grpc-server"
export OTEL_TRACES_EXPORTER="console"
export OTEL_LOGS_EXPORTER="console"
export OTEL_PROPAGATORS="tracecontext,baggage"
export OTEL_BSP_SCHEDULE_DELAY="1"
export OTEL_BLRP_SCHEDULE_DELAY="1"
export RUST_LOG="info"

exec cargo run --bin server
