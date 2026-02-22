import threading

import pytest
import grpc

from opentelemetry import trace
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import SimpleSpanProcessor, SpanExporter, SpanExportResult

from opentelemetry.instrumentation.grpc import (
    GrpcAioInstrumentorServer,
    GrpcAioInstrumentorClient,
)

import helloworld_pb2
import helloworld_pb2_grpc


class InMemorySpanExporter(SpanExporter):
    """Collects spans in memory for test assertions."""

    def __init__(self):
        self._spans = []
        self._lock = threading.Lock()

    def export(self, spans):
        with self._lock:
            self._spans.extend(spans)
        return SpanExportResult.SUCCESS

    def get_finished_spans(self):
        with self._lock:
            return list(self._spans)

    def clear(self):
        with self._lock:
            self._spans.clear()

    def shutdown(self):
        self.clear()


# --- Module-level OTel setup (runs once at import time) ---

_exporter = InMemorySpanExporter()
_provider = TracerProvider()
_provider.add_span_processor(SimpleSpanProcessor(_exporter))
trace.set_tracer_provider(_provider)

GrpcAioInstrumentorServer().instrument()
GrpcAioInstrumentorClient().instrument()


# --- Fixtures ---


@pytest.fixture(scope="session")
def exporter():
    return _exporter


@pytest.fixture(autouse=True)
def clear_spans(exporter):
    exporter.clear()


class GreeterServicer(helloworld_pb2_grpc.GreeterServicer):
    async def SayHello(self, request, context):
        return helloworld_pb2.HelloReply(message=f"Hello, {request.name}!")


@pytest.fixture
async def grpc_server():
    server = grpc.aio.server()
    helloworld_pb2_grpc.add_GreeterServicer_to_server(GreeterServicer(), server)
    port = server.add_insecure_port("[::]:0")
    await server.start()
    yield port
    await server.stop(grace=0)
