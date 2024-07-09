use containerd_client::{connect, services::v1::version_client::VersionClient};

async fn query_version() {
    // Launch containerd at /run/containerd/containerd.sock
    let channel = connect("/run/containerd/containerd.sock").await.unwrap();

    let mut client = VersionClient::new(channel);
    let resp = client.version(()).await.unwrap();

    println!("Response: {:?}", resp.get_ref());
}

#[tokio::main]
async fn main() {
    query_version().await;
    println!("Hello, world!");
}
