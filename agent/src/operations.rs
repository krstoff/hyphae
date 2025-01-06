use cri::image_service_client::ImageServiceClient;
use cri::runtime_service_client::RuntimeServiceClient;
use k8s_cri::v1::{self as cri, Container, ContainerMetadata, KeyValue, LinuxContainerResources, LinuxSandboxSecurityContext, StopPodSandboxRequest};
use tonic::transport::Channel;
use std::collections::HashMap;
use std::sync::Mutex;

use crate::state::State;

pub struct SandBoxConfig {
    pub name: String,
    pub uid: String,
    pub resources: Option<cri::LinuxContainerResources>,
    pub namespace: String,
}

pub async fn pull_image(isc: &mut ImageServiceClient<Channel>, name: String) -> cri::PullImageResponse {
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

pub async fn create_sandbox(rsc: &mut RuntimeServiceClient<Channel>, config: SandBoxConfig) -> (cri::RunPodSandboxResponse, cri::PodSandboxConfig) {
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
        log_directory: "/var/log/pods/".to_owned() + &config.name,
        port_mappings: vec![],
        annotations: HashMap::new(),
        windows: None,
    };
    let request = cri::RunPodSandboxRequest {
        config: Some(config.clone()),
        runtime_handler: String::new(),
    };
    return (rsc.run_pod_sandbox(request)
        .await
        .expect("Sandbox creation failed")
        .into_inner(), config);
}

#[derive(Clone)]
pub struct ContainerConfig {
    pub pod_sandbox_id: String,
    pub name: String,
    pub image: String,
    pub command: String,
    pub args: Vec<String>,
    pub working_dir: String,
    pub envs: Vec<(String, String)>,
    pub privileged: bool,
}

pub async fn run_container(rsc: &mut RuntimeServiceClient<Channel>, config: ContainerConfig, sandbox_config: cri::PodSandboxConfig)
    -> (cri::StartContainerResponse, String)
{
    let container_labels = HashMap::from([
        ("name".to_owned(), config.name.clone()),
    ]);
    let linux_options = cri::LinuxContainerConfig {
        resources: None,
        security_context: Some(cri::LinuxContainerSecurityContext {
            privileged: config.privileged,
            namespace_options: Some(cri::NamespaceOption {
                network: cri::NamespaceMode::Node.into(),
                ..Default::default()
            }),
            ..Default::default()   
        }),
    };
    let cri_container_config = cri::ContainerConfig {
        metadata: Some(ContainerMetadata {
            name: config.name.clone(),
            attempt: 0,
        }),
        image: Some(cri::ImageSpec {
            image: config.image,
            annotations: HashMap::new(),
        }),
        command: vec![config.command],
        args: config.args,
        working_dir: config.working_dir,
        envs: config.envs.into_iter().map(|(key, value)| cri::KeyValue { key, value }).collect(),
        labels: container_labels,
        annotations: HashMap::new(),
        log_path: config.name.clone() + &"-id.log", // TODO: Fix this
        linux: Some(linux_options),
        stdin_once: false,
        stdin: false,
        tty: false,
        mounts: vec![],
        devices: vec![],
        windows: None,
    };
    let create_request = cri::CreateContainerRequest {
        pod_sandbox_id: config.pod_sandbox_id,
        config: Some(cri_container_config),
        sandbox_config: Some(sandbox_config),
    };
    
    let create_resp = rsc.create_container(create_request)
        .await
        .expect("Container creation failed")
        .into_inner();

    // Start the container
    let start_request = cri::StartContainerRequest { container_id: create_resp.container_id.clone()};
    let start_resp = rsc.start_container(start_request)
        .await
        .expect("Container start failed")
        .into_inner();

    return (start_resp, create_resp.container_id);
}

pub async fn stop_container(rsc: &mut RuntimeServiceClient<Channel>, container_id: String) -> cri::StopContainerResponse {
    let stop_req = cri::StopContainerRequest {
        container_id: container_id,
        timeout: 0,
    };
    let stop_resp = rsc.stop_container(stop_req)
        .await
        .expect("Container stop failed")
        .into_inner();
    return stop_resp;
}

pub async fn remove_container(rsc: &mut RuntimeServiceClient<Channel>, container_id: String) -> cri::RemoveContainerResponse {
    let remove_req = cri::RemoveContainerRequest {
        container_id: container_id,
    };
    let remove_resp = rsc.remove_container(remove_req)
        .await
        .expect("Container stop failed")
        .into_inner();
    return remove_resp;
}

pub async fn remove_pod(rsc: &mut RuntimeServiceClient<Channel>, pod_id: String) -> cri::RemovePodSandboxResponse {
    let stop_req = cri::StopPodSandboxRequest {
        pod_sandbox_id: pod_id.clone()
    };
    let stop_resp = rsc.stop_pod_sandbox(stop_req)
        .await
        .expect("Stopping pod failed")
        .into_inner();

    let remove_req = cri::RemovePodSandboxRequest {
        pod_sandbox_id: pod_id.clone()
    };
    let remove_resp = rsc.remove_pod_sandbox(remove_req)
        .await
        .expect("Pod removal failed")
        .into_inner();

    return remove_resp;
}

pub async fn list_containers(rsc: &mut RuntimeServiceClient<Channel>) -> cri::ListContainersResponse {
    let list_req = cri::ListContainersRequest {
        filter: None
    };
    rsc.list_containers(list_req)
        .await
        .expect("Listing containers failed")
        .into_inner()
    
}