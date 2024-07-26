mod operations;
mod state;
#[cfg(test)]
mod tests;

use k8s_cri::v1 as cri;
use cri::image_service_client::ImageServiceClient;
use cri::runtime_service_client::RuntimeServiceClient;

use std::sync::{Arc, Mutex};

use operations::*;
use state::*;

async fn connect_uds(path: String) -> Result<tonic::transport::Channel, tonic::transport::Error> {
    tonic::transport::Endpoint::try_from("http://[::]:50051")
    .unwrap()
    .connect_with_connector(
        tower::service_fn(move |_| {
            tokio::net::UnixStream::connect(path.clone())
        })
    ).await
}

async fn read_events(mut runtime_service: RuntimeServiceClient<tonic::transport::Channel>, runtime_state: Arc<Mutex<State>>) {
    loop {
        let mut events_resp = runtime_service.get_container_events(cri::GetEventsRequest{})
            .await
            .expect("Could not get event stream")
            .into_inner();
        while let Ok(Some(message)) = events_resp.message().await {
            runtime_state.lock().unwrap().process_message(message);
        }
    }
}

#[tokio::main]
async fn main() {
    let channel = connect_uds("/run/containerd/containerd.sock".to_owned()).await.expect("Could not connect to containerd");
    let mut runtime_service = RuntimeServiceClient::new(channel.clone());

    let runtime_state = State::new();

    let events_process = tokio::spawn(read_events(runtime_service, runtime_state));
    // let containers = list_containers(&mut runtime_service).await.containers;
    // runtime_state.lock().unwrap().observe(containers);
    events_process.await;
    
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    }
}