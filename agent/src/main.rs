mod operations;
mod state;
#[cfg(test)]
mod tests;

use k8s_cri::v1 as cri;
use cri::image_service_client::ImageServiceClient;
use cri::runtime_service_client::RuntimeServiceClient;
use tokio::runtime;

use std::{sync::{Arc, Mutex}, time::Duration};

use operations::*;
use state::*;

const CLEANUP_INTERVAL_MS: u64 = 10_000;

async fn connect_uds(path: String) -> Result<tonic::transport::Channel, tonic::transport::Error> {
    tonic::transport::Endpoint::try_from("http://[::]:50051")
    .unwrap()
    .connect_with_connector(
        tower::service_fn(move |_| {
            tokio::net::UnixStream::connect(path.clone())
        })
    ).await
}

async fn read_events(mut runtime_service: RuntimeServiceClient<tonic::transport::Channel>, mut runtime_state: StateHandle) {
    loop {
        read_all_messages(&mut runtime_service, &mut runtime_state).await;
        tokio::time::sleep(Duration::from_millis(1_000)).await;
    }
}

async fn read_all_messages(runtime_service: &mut RuntimeServiceClient<tonic::transport::Channel>, runtime_state: &mut StateHandle) {
    let mut events_resp = runtime_service.get_container_events(cri::GetEventsRequest {})
        .await
        .expect("Could not get events stream.")
        .into_inner();
    let mut state = runtime_state.lock().await;
    while let Ok(Some(message)) = events_resp.message().await {
        state.process_message(message);
    }
}

#[tokio::main]
async fn main() {
    let channel = connect_uds("/run/containerd/containerd.sock".to_owned()).await.expect("Could not connect to containerd");
    let mut runtime_service = RuntimeServiceClient::new(channel.clone());

    let runtime_state = State::new();

    let events_process = tokio::spawn(read_events(runtime_service.clone(), runtime_state.clone()));
    // let containers = list_containers(&mut runtime_service).await.containers;
    // runtime_state.lock().unwrap().observe(containers);

    setup_teardown().await;
    
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    }
}

pub async fn setup_teardown() {
    let channel = connect_uds("/run/containerd/containerd.sock".to_owned()).await.expect("Could not connect to containerd");
    let mut image_service = ImageServiceClient::new(channel.clone());
    let image_name = "docker.io/library/nginx:latest".to_owned();
    let pull_image_response = pull_image(&mut image_service, image_name).await;
    println!("{:?}", &pull_image_response);
    let mut runtime_service = RuntimeServiceClient::new(channel);

    fn make_uid() -> String {
        return "123456789".to_owned();
    }
    let uid = make_uid();

    let sandbox_config = SandBoxConfig {
        name: "nginx".to_owned(),
        uid: uid,
        namespace: "default".to_owned(),
        resources: None,
    };
    let (create_sandbox_response, podsandbox_config) = create_sandbox(&mut runtime_service, sandbox_config).await;
    println!("{:?}", &create_sandbox_response);

    let container_config = ContainerConfig {
        pod_sandbox_id: create_sandbox_response.pod_sandbox_id.clone(),
        name: "nginx-container".to_owned(),
        image: pull_image_response.image_ref.clone(),
        command: "nginx".to_owned(),
        args: vec![],
        working_dir: "".to_owned(),
        envs: vec![],
        privileged: false
    };
    let run_container_response = run_container(&mut runtime_service, container_config, podsandbox_config).await;
    println!("{:?}", &run_container_response);

    tokio::time::sleep(std::time::Duration::from_millis(5000)).await;

    //////////////////////
    let containers = list_containers(&mut runtime_service).await.containers;
    let pods = list_pods(&mut runtime_service).await.items;
    dbg!(containers);
    dbg!(pods);
}