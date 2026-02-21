from opentelemetry.instrumentation.auto_instrumentation import initialize

initialize()

import asyncio
import logging

import grpc

import helloworld_pb2
import helloworld_pb2_grpc

logger = logging.getLogger(__name__)


class GreeterServicer(helloworld_pb2_grpc.GreeterServicer):
    async def SayHello(self, request, context):
        logger.info("Received request: name=%s", request.name)
        return helloworld_pb2.HelloReply(message=f"Hello, {request.name}!")


async def serve():
    server = grpc.aio.server()
    helloworld_pb2_grpc.add_GreeterServicer_to_server(GreeterServicer(), server)
    server.add_insecure_port("[::]:50051")
    logger.info("Server starting on port 50051")
    await server.start()
    logger.info("Server started")
    await server.wait_for_termination()


if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO)
    asyncio.run(serve())
