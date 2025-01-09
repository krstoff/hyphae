mod control_loop;
mod operations;
mod state;
#[cfg(test)]
mod tests;

use k8s_cri::v1 as cri;
use cri::image_service_client::ImageServiceClient;
use cri::runtime_service_client::RuntimeServiceClient;
use tokio::runtime;

use std::time::Duration;

use operations::*;
use state::*;

const CONTAINERD_SOCKET_PATH: &'static str = "/run/containerd/containerd.sock";

async fn connect_uds() -> Result<tonic::transport::Channel, tonic::transport::Error> {
    use hyper_util::rt::TokioIo;
    use tokio::net::UnixStream;
    tonic::transport::Endpoint::try_from("http://[::]:50051")?
    .connect_with_connector(
        tower::service_fn(move |_| async {
            Ok::<_, std::io::Error>(TokioIo::new(UnixStream::connect(CONTAINERD_SOCKET_PATH).await?))
        })
    ).await
}

async fn read_events(mut runtime_service: RuntimeServiceClient<tonic::transport::Channel>, mut runtime_state: StateHandle) {
    loop {
        read_all_messages(&mut runtime_service, &mut runtime_state).await;
        tokio::time::sleep(Duration::from_millis(2_000)).await;
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
    let channel = connect_uds().await.expect("Could not connect to containerd");
    let mut runtime_service = RuntimeServiceClient::new(channel.clone());

    let runtime_state = state::State::new();
    let target_state = control_loop::Target::new();

    // let events_process = tokio::spawn(read_events(runtime_service.clone(), runtime_state.clone()));
    // let control_process = tokio::spawn(control_loop::control_loop(runtime_service.clone(), runtime_state.clone(), target_state.clone()));
    // let containers = list_containers(&mut runtime_service).await.containers;

    setup_teardown(runtime_state.clone()).await;
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    }
}

async fn setup_teardown(state: StateHandle) {
    let channel = connect_uds().await.expect("Could not connect to containerd");
    let mut image_service = ImageServiceClient::new(channel.clone());
    let image_name = "docker.io/library/nginx:latest".to_owned();
    let pull_image_response = pull_image(&mut image_service, image_name).await.unwrap();
    let mut runtime_service = RuntimeServiceClient::new(channel);

    fn make_uid() -> String {
        return "123456789".to_owned();
    }
    let uid = make_uid();

    let sandbox_config = SandBoxConfig {
        name: "nginx".to_owned(),
        uid: uid.clone(),
        namespace: "default".to_owned(),
        resources: None,
    };
    let (pod_id, _) = create_sandbox(&mut runtime_service, sandbox_config.clone()).await.unwrap();

    let container_config = ContainerConfig {
        name: "nginx-container".to_owned(),
        pod_uid: uid.clone(),
        image: pull_image_response.image_ref.clone(),
        command: "nginx".to_owned(),
        args: vec![],
        working_dir: "".to_owned(),
        envs: vec![],
        privileged: false
    };
    let container_id = create_container(&mut runtime_service, pod_id.clone(), container_config, sandbox_config.clone()).await.unwrap();

    
    let container_config2 = ContainerConfig {
        name: "nginx-container2".to_owned(),
        pod_uid: uid,
        image: pull_image_response.image_ref.clone(),
        command: "nginx".to_owned(),
        args: vec![],
        working_dir: "".to_owned(),
        envs: vec![],
        privileged: false
    };
    let container_id2 = create_container(&mut runtime_service, pod_id.clone(), container_config2, sandbox_config).await.unwrap();
    println!("Container created: {:?}", &container_id);

    start_container(&mut runtime_service, container_id2).await.unwrap();
    start_container(&mut runtime_service, container_id).await.unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(5000)).await;

    remove_pod(&mut runtime_service, pod_id.clone()).await;
    
    // //////////////////////
    // let containers = list_containers(&mut runtime_service).await.unwrap().containers;
    // let pods = list_pods(&mut runtime_service).await.unwrap().items;
    // dbg!(containers);
    // dbg!(pods);
}