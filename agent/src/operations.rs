use cri::image_service_client::ImageServiceClient;
use cri::runtime_service_client::RuntimeServiceClient;
use k8s_cri::v1::{self as cri, Container, ContainerMetadata, KeyValue, LinuxContainerResources, LinuxSandboxSecurityContext};
use tonic::transport::Channel;
use std::collections::HashMap;

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
    -> cri::StartContainerResponse
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
        // command: vec![config.command],
        // args: config.args,
        command: vec!["/bin/bash".to_owned()],
        args: vec!["-c".to_owned(), "while true; do echo $(date); sleep 2; done".to_owned()],
        working_dir: config.working_dir,
        envs: config.envs.into_iter().map(|(key, value)| cri::KeyValue { key, value }).collect(),
        labels: container_labels,
        annotations: HashMap::new(),
        log_path: config.name.clone() + &"/id.log",
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
    let start_request = cri::StartContainerRequest { container_id: create_resp.container_id };
    let start_resp = rsc.start_container(start_request)
        .await
        .expect("Container start failed")
        .into_inner();

    return start_resp;
}