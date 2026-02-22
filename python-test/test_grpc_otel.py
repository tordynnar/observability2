import grpc

import helloworld_pb2
import helloworld_pb2_grpc
from opentelemetry.trace import StatusCode


async def test_say_hello_produces_distributed_trace(grpc_server, exporter):
    port = grpc_server

    async with grpc.aio.insecure_channel(f"localhost:{port}") as channel:
        stub = helloworld_pb2_grpc.GreeterStub(channel)
        response = await stub.SayHello(helloworld_pb2.HelloRequest(name="Test"))

    assert response.message == "Hello, Test!"

    spans = exporter.get_finished_spans()
    assert len(spans) >= 2, f"Expected >= 2 spans, got {len(spans)}"

    # All spans share the same trace_id (distributed tracing works)
    trace_ids = {span.context.trace_id for span in spans}
    assert len(trace_ids) == 1, f"Expected 1 trace_id, got {trace_ids}"

    # Find client and server spans
    client_spans = [s for s in spans if s.kind.name == "CLIENT"]
    server_spans = [s for s in spans if s.kind.name == "SERVER"]
    assert len(client_spans) >= 1, "Expected at least 1 CLIENT span"
    assert len(server_spans) >= 1, "Expected at least 1 SERVER span"

    # Span names contain "SayHello"
    for span in spans:
        assert "SayHello" in span.name, f"Expected 'SayHello' in span name, got '{span.name}'"

    # Server span's parent is the client span
    client_span = client_spans[0]
    server_span = server_spans[0]
    assert server_span.parent is not None, "Server span should have a parent"
    assert server_span.parent.span_id == client_span.context.span_id
