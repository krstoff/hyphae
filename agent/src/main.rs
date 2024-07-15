use k8s_cri::v1::{self as cri, LinuxContainerResources, LinuxSandboxSecurityContext};
use cri::image_service_client::ImageServiceClient;
use cri::runtime_service_client::RuntimeServiceClient;
use tonic::transport::Channel;
use std::collections::HashMap;

async fn connect_uds(path: String) -> Result<tonic::transport::Channel, tonic::transport::Error> {
    tonic::transport::Endpoint::try_from("http://[::]:50051")
    .unwrap()
    .connect_with_connector(
        tower::service_fn(move |_| {
            tokio::net::UnixStream::connect(path.clone())
        })
    ).await
}

struct SandBoxConfig {
    name: String,
    uid: String,
    resources: Option<cri::LinuxContainerResources>,
    namespace: String,
}

async fn pull_image(isc: &mut ImageServiceClient<Channel>, name: String) -> cri::PullImageResponse {
    isc.pull_image(cri::PullImageRequest{
        image: Some(cri:: ImageSpec {
            image: name,
            annotations: Default::default()
        }),
        auth: None,
        sandbox_config: None,
    })
        .await
        .expect("Could not pull image")
        .into_inner()
}

async fn create_sandbox(rsc: &mut RuntimeServiceClient<Channel>, config: SandBoxConfig) -> cri::RunPodSandboxResponse {
    let metadata = cri::PodSandboxMetadata {
        name: config.name.clone(),
        uid: config.uid,
        namespace: config.namespace,
        attempt: 0,
    };
    let sandbox_labels = HashMap::from([
        ("name".to_owned(), config.name.clone()),
    ]);
    let linux_options = cri::LinuxPodSandboxConfig {
        cgroup_parent: "".to_owned(),
        resources: config.resources,
        security_context: Some(cri::LinuxSandboxSecurityContext {
            namespace_options: Some(cri::NamespaceOption {
                network: cri::NamespaceMode::Node.into(),
                ..Default::default()
            }),
            ..Default::default()   
        }),
        overhead: None,
        sysctls: HashMap::new(),
    };
    let config = cri::PodSandboxConfig {
        metadata: Some(metadata),
        labels: sandbox_labels,
        linux: Some(linux_options),
        hostname: String::new(),
        dns_config: None,
        log_directory: String::new(),
        port_mappings: vec![],
        annotations: HashMap::new(),
        windows: None,
    };
    let request = cri::RunPodSandboxRequest {
        config: Some(config),
        runtime_handler: String::new(),
    };
    return rsc.run_pod_sandbox(request)
        .await
        .expect("Sandbox creation failed")
        .into_inner();
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
    let config = SandBoxConfig {
        name: "nginx".to_owned(),
        uid: uid,
        namespace: "default".to_owned(),
        resources: None,
    };
    let create_sandbox_response = create_sandbox(&mut runtime_service, config).await;
    println!("{:?}", &create_sandbox_response);
}