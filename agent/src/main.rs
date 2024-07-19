use k8s_cri::v1 as cri;
use cri::image_service_client::ImageServiceClient;
use cri::runtime_service_client::RuntimeServiceClient;
mod operations;
use operations::*;

async fn connect_uds(path: String) -> Result<tonic::transport::Channel, tonic::transport::Error> {
    tonic::transport::Endpoint::try_from("http://[::]:50051")
    .unwrap()
    .connect_with_connector(
        tower::service_fn(move |_| {
            tokio::net::UnixStream::connect(path.clone())
        })
    ).await
}

#[tokio::main]
async fn main() {
    let channel = connect_uds("/run/containerd/containerd.sock".to_owned()).await.expect("Could not connect to containerd");
    let mut image_service = ImageServiceClient::new(channel.clone());
    let image_name = "docker.io/library/nginx:latest".to_owned();
    let pull_image_response = pull_image(&mut image_service, image_name).await;
    println!("{:?}", &pull_image_response);

    fn make_uid() -> String {
        return "testuid".to_owned();
    }
    let uid = make_uid();
    let mut runtime_service = RuntimeServiceClient::new(channel);
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
        name: "nginx".to_owned(),
        image: pull_image_response.image_ref.clone(),
        command: "nginx".to_owned(),
        args: vec![],
        working_dir: "".to_owned(),
        envs: vec![],
        privileged: false
    };
    let create_container_response = create_container(&mut runtime_service, container_config, podsandbox_config).await;
    println!("{:?}", &create_container_response);
}