# Cancelling Async gRPC Server-Streaming Responses

## How to cancel a stream

OpenTelemetry instrumentation wraps gRPC streams in an async generator, hiding the `.cancel()` method. Use this helper:

```python
import types

async def cancel_stream(stream):
    if hasattr(stream, "cancel"):
        stream.cancel()
    elif isinstance(stream, types.AsyncGeneratorType):
        await stream.aclose()
    else:
        raise TypeError(f"Don't know how to cancel {type(stream)}")
```

Usage:

```python
stream = stub.ServerStreamingMethod(request)
async for response in stream:
    if should_stop(response):
        await cancel_stream(stream)
        break
```

## Troubleshooting

**`AttributeError: 'async_generator' object has no attribute 'cancel'`**

You called `.cancel()` on an instrumented stream. The OTel gRPC instrumentation (`opentelemetry-instrumentation-grpc` 0.60b1 and earlier) wraps the stream in an async generator via `async for` / `yield`, which strips the underlying gRPC object's methods. Use `cancel_stream()` above instead of calling `.cancel()` directly.

**`grpc.aio.AioRpcError` with `CANCELLED` after cancelling**

If you continue the `async for` loop after calling `cancel_stream()`, the behavior depends on whether the stream is instrumented: a raw gRPC stream raises `AioRpcError(CANCELLED)` on the next iteration, while an instrumented async generator silently exits the loop (since `aclose()` closes the generator, causing `StopAsyncIteration`). Wrap the loop in `try`/`except grpc.aio.AioRpcError` and check for `StatusCode.CANCELLED` to handle this consistently.

**Stream cancelled but span never ends**

If you break out of an `async for` loop without calling `aclose()`, the async generator may not finalize. The instrumentation's `finally: span.end()` block only runs when the generator is properly closed. Always use `cancel_stream()` or ensure the generator is closed (e.g., via `async with aclosing(stream)`).

## Upstream fix

Bug: [opentelemetry-python-contrib#2014](https://github.com/open-telemetry/opentelemetry-python-contrib/issues/2014). Fix PRs: [#3823](https://github.com/open-telemetry/opentelemetry-python-contrib/pull/3823), [#2093](https://github.com/open-telemetry/opentelemetry-python-contrib/pull/2093). Remove the `cancel_stream` helper once a fix is released.
