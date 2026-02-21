use tonic::Request;
use tower::ServiceBuilder;

use observability2_rust::pb;
use observability2_rust::telemetry;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let guard = telemetry::init();

    let channel = tonic::transport::Channel::from_static("http://localhost:50051")
        .connect()
        .await?;
    let channel = ServiceBuilder::new()
        .layer(tonic_tracing_opentelemetry::middleware::client::OtelGrpcLayer)
        .service(channel);

    let mut client = pb::greeter_client::GreeterClient::new(channel);

    let response = client
        .say_hello(Request::new(pb::HelloRequest {
            name: "World".into(),
        }))
        .await?;

    tracing::info!(message = %response.into_inner().message, "Greeter response");

    guard.shutdown();
    Ok(())
}
