use tonic::transport::Server;
use tonic::{Request, Response, Status};

use observability2_rust::pb;
use observability2_rust::telemetry;

struct GreeterService;

#[tonic::async_trait]
impl pb::greeter_server::Greeter for GreeterService {
    async fn say_hello(
        &self,
        request: Request<pb::HelloRequest>,
    ) -> Result<Response<pb::HelloReply>, Status> {
        let name = request.into_inner().name;
        tracing::info!(name = %name, "Received request");
        Ok(Response::new(pb::HelloReply {
            message: format!("Hello, {}!", name),
        }))
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
        .layer(tonic_tracing_opentelemetry::middleware::server::OtelGrpcLayer::default())
        .add_service(pb::greeter_server::GreeterServer::new(GreeterService))
        .serve_with_shutdown(addr, shutdown_signal())
        .await?;

    drop(guard);
    Ok(())
}
