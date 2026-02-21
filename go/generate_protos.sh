#!/usr/bin/env bash
set -euo pipefail

# Generate Go code from proto files.
#
# Prerequisites:
#   go install google.golang.org/protobuf/cmd/protoc-gen-go@latest
#   go install google.golang.org/grpc/cmd/protoc-gen-go-grpc@latest
#
# Both binaries must be on your PATH (typically ~/go/bin).

export PATH="$HOME/go/bin:$PATH"

PROTO_DIR="protos"
OUT_DIR="."

protoc \
  -I"$PROTO_DIR" \
  --go_out="$OUT_DIR" \
  --go_opt=module=observability2/go \
  --go-grpc_out="$OUT_DIR" \
  --go-grpc_opt=module=observability2/go \
  "$PROTO_DIR/helloworld.proto"

echo "Proto generation complete."
