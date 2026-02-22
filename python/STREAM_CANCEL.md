# gRPC Server-Streaming Cancel Bug (OpenTelemetry Instrumentation)

## The Problem

`opentelemetry-instrumentation-grpc` wraps server-streaming responses in a
plain Python generator via `yield from`. This hides the underlying
`_MultiThreadedRendezvous` object and its `.cancel()` method.

The relevant code in `_client.py:195-220`:

```python
def _intercept_server_stream(
    self, request_or_iterator, metadata, client_info, invoker
):
    if not metadata:
        mutable_metadata = OrderedDict()
    else:
        mutable_metadata = OrderedDict(metadata)

    with self._start_span(client_info.full_method) as span:
        inject(mutable_metadata, setter=_carrier_setter)
        metadata = tuple(mutable_metadata.items())
        rpc_info = RpcInfo(
            full_method=client_info.full_method,
            metadata=metadata,
            timeout=client_info.timeout,
        )

        if client_info.is_client_stream:
            rpc_info.request = request_or_iterator

        try:
            yield from invoker(request_or_iterator, metadata)
        except grpc.RpcError as err:
            span.set_status(Status(StatusCode.ERROR))
            span.set_attribute(RPC_GRPC_STATUS_CODE, err.code().value[0])
            raise err
```

Because `_intercept_server_stream` is a generator function (it contains
`yield from`), calling it returns a `generator` object — not the gRPC
`_MultiThreadedRendezvous` that `invoker()` would normally return.

## What Breaks

```python
stub = MyServiceStub(channel)
stream = stub.ServerStreamingMethod(request)

# Without instrumentation: stream is _MultiThreadedRendezvous, has .cancel()
# With instrumentation:    stream is a generator, lacks .cancel()

stream.cancel()
# AttributeError: 'generator' object has no attribute 'cancel'
```

## Workaround

Use a helper that detects whether the stream is a raw gRPC object or an
instrumentation-wrapped generator:

```python
import types

def cancel_stream(stream):
    """Cancel a gRPC server-streaming response, whether or not it's
    wrapped by OpenTelemetry instrumentation."""
    if hasattr(stream, "cancel"):
        # Raw gRPC stream (_MultiThreadedRendezvous)
        stream.cancel()
    elif isinstance(stream, types.GeneratorType):
        # Instrumented stream — .close() triggers GeneratorExit inside
        # the generator, which exits the `with` context manager in
        # _intercept_server_stream, cleanly ending the span.
        stream.close()
    else:
        raise TypeError(f"Don't know how to cancel {type(stream)}")
```

Usage:

```python
stream = stub.ServerStreamingMethod(request)
for response in stream:
    if should_stop(response):
        cancel_stream(stream)
        break
```

## Async Variant

The async instrumentation path (`_aio_client.py:128-138`) has the same
shape:

```python
async def _wrap_stream_response(self, span, call):
    try:
        async for response in call:
            if self._response_hook:
                self._call_response_hook(span, response)
            yield response
    except Exception as exc:
        self.add_error_details_to_span(span, exc)
        raise exc
    finally:
        span.end()
```

This wraps the stream in an async generator. Use `aclose()` instead of
`cancel()`:

```python
import types

async def cancel_stream_async(stream):
    """Cancel an async gRPC server-streaming response."""
    if hasattr(stream, "cancel"):
        stream.cancel()
    elif isinstance(stream, types.AsyncGeneratorType):
        # aclose() triggers GeneratorExit in the async generator,
        # which hits the finally block and calls span.end().
        await stream.aclose()
    else:
        raise TypeError(f"Don't know how to cancel {type(stream)}")
```

## Upstream Status

This bug is tracked upstream. The workaround is needed for version **0.60b1
and earlier**.

- Issue: [open-telemetry/opentelemetry-python-contrib#2014](https://github.com/open-telemetry/opentelemetry-python-contrib/issues/2014)
- Fix PR (wrapper class approach): [#3823](https://github.com/open-telemetry/opentelemetry-python-contrib/pull/3823)
- Fix PR (earlier attempt): [#2093](https://github.com/open-telemetry/opentelemetry-python-contrib/pull/2093)

Once an upstream fix is released and the dependency is updated, the
`cancel_stream` / `cancel_stream_async` helpers can be removed in favor of
calling `.cancel()` directly.
