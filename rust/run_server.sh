#!/usr/bin/env bash
set -euo pipefail

export OTEL_SERVICE_NAME="grpc-server"

# Rust's stdout exporter is hardcoded (no autoexport equivalent).
# These vars are set for documentation consistency; OTEL_SERVICE_NAME is read by the code.
export OTEL_TRACES_EXPORTER="console"
export OTEL_LOGS_EXPORTER="console"
export OTEL_PROPAGATORS="tracecontext,baggage"

# Batch processor delays — 1ms for immediate output during development.
# Read by the SDK's BatchConfigBuilder via init_from_env_vars().
export OTEL_BSP_SCHEDULE_DELAY="1"
export OTEL_BLRP_SCHEDULE_DELAY="1"

# Set the tracing filter so INFO-level events are emitted.
export RUST_LOG="info"

exec cargo run --bin server
