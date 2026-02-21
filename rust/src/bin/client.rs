use tonic::Request;
use tracing::Instrument;

use observability2_rust::pb;
use observability2_rust::telemetry;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let guard = telemetry::init();

    let mut client =
        pb::greeter_client::GreeterClient::connect("http://localhost:50051").await?;

    let span = tracing::info_span!(
        "helloworld.Greeter/SayHello",
        otel.kind = "client",
        rpc.system = "grpc",
        rpc.service = "helloworld.Greeter",
        rpc.method = "SayHello",
    );

    async {
        let mut request = Request::new(pb::HelloRequest {
            name: "World".into(),
        });
        telemetry::inject_trace_context(request.metadata_mut());
        let response = client.say_hello(request).await?;
        tracing::info!(message = %response.into_inner().message, "Greeter response");
        Ok::<(), Box<dyn std::error::Error>>(())
    }
    .instrument(span)
    .await?;

    guard.shutdown();
    Ok(())
}
