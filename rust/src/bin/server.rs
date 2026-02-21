use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;

use observability2_rust::pb;
use observability2_rust::telemetry;

struct GreeterService;

#[tonic::async_trait]
impl pb::greeter_server::Greeter for GreeterService {
    async fn say_hello(
        &self,
        request: Request<pb::HelloRequest>,
    ) -> Result<Response<pb::HelloReply>, Status> {
        // Extract parent context from incoming gRPC metadata.
        let parent_cx = telemetry::extract_trace_context(request.metadata());
        let name = request.into_inner().name;

        let span = tracing::info_span!(
            "helloworld.Greeter/SayHello",
            otel.kind = "server",
            rpc.system = "grpc",
            rpc.service = "helloworld.Greeter",
            rpc.method = "SayHello",
        );
        let _ = span.set_parent(parent_cx);

        async {
            tracing::info!(name = %name, "Received request");
            Ok(Response::new(pb::HelloReply {
                message: format!("Hello, {}!", name),
            }))
        }
        .instrument(span)
        .await
    }
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for ctrl+c");
    tracing::info!("Shutting down server");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let guard = telemetry::init();

    let addr = "[::]:50051".parse()?;
    tracing::info!("Server starting on port 50051");

    Server::builder()
        .add_service(pb::greeter_server::GreeterServer::new(GreeterService))
        .serve_with_shutdown(addr, shutdown_signal())
        .await?;

    guard.shutdown();
    Ok(())
}
