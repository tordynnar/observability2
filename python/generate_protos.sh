#!/usr/bin/env bash
set -euo pipefail

PROTO_DIR="protos"
OUT_DIR="."

uv run --group dev python -m grpc_tools.protoc \
  -I"$PROTO_DIR" \
  --python_out="$OUT_DIR" \
  --grpc_python_out="$OUT_DIR" \
  --pyi_out="$OUT_DIR" \
  "$PROTO_DIR/helloworld.proto"

echo "Proto generation complete."
