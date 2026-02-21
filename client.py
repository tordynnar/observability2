from opentelemetry.instrumentation.auto_instrumentation import initialize

initialize()

import asyncio
import logging

import grpc

import helloworld_pb2
import helloworld_pb2_grpc

logger = logging.getLogger(__name__)


async def run():
    async with grpc.aio.insecure_channel("localhost:50051") as channel:
        stub = helloworld_pb2_grpc.GreeterStub(channel)
        response = await stub.SayHello(helloworld_pb2.HelloRequest(name="World"))
        logger.info("Greeter response: %s", response.message)


if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO)
    asyncio.run(run())
