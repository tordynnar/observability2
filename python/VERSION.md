# gRPC Version Constraints

The lower bounds for `grpcio` and `grpcio-tools` in `pyproject.toml` must match
the version of `grpcio-tools` used to generate the proto bindings. This is
because the generated code contains a runtime version check that will reject an
older `grpcio` package.

## How it works

The generated gRPC stub files (`*_pb2_grpc.py`) contain:

```python
GRPC_GENERATED_VERSION = '1.78.1'
```

At import time, it compares the installed `grpc.__version__` against this value.
If the installed version is lower, it raises a `RuntimeError`:

> The grpc package installed is at version {X}, but the generated code depends
> on grpcio>={GRPC_GENERATED_VERSION}.

This means `grpcio>=1.78.1` must be specified in `pyproject.toml` dependencies.

## Maintaining the bounds

When regenerating the proto bindings:

1. Check `GRPC_GENERATED_VERSION` in all `*_pb2_grpc.py` files.
2. Find the highest version among them.
3. Update `pyproject.toml` so the lower bounds for `grpcio` (and `grpcio-tools`
   in dev dependencies) are `>=` that highest version.
