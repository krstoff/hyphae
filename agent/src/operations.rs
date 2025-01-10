use cri::image_service_client::ImageServiceClient;
use cri::runtime_service_client::RuntimeServiceClient;
use k8s_cri::v1::{self as cri, Container, ContainerMetadata, KeyValue, LinuxContainerResources, LinuxSandboxSecurityContext, StopPodSandboxRequest};
use tonic::{transport::Channel, Status};
use std::collections::HashMap;

#[derive(Clone)]
pub struct SandBoxConfig {
    pub name: String,
    pub uid: String,
    pub resources: Option<cri::LinuxContainerResources>,
    pub namespace: String,
}

#[derive(Clone)]
pub struct ContainerConfig {
    pub pod_uid: String,
    pub name: String,
    pub image: String,
    pub command: String,
    pub args: Vec<String>,
    pub working_dir: String,
    pub envs: Vec<(String, String)>,
    pub privileged: bool,
}

impl SandBoxConfig {
    pub fn to_cri_config(self) -> cri::PodSandboxConfig {
        let metadata = cri::PodSandboxMetadata {
            name: self.name.clone(),
            uid: self.uid,
            namespace: self.namespace,
            attempt: 0,
        };
        let sandbox_labels = HashMap::from([
            ("name".to_owned(), self.name.clone()),
        ]);
        let linux_options = cri::LinuxPodSandboxConfig {
            cgroup_parent: "".to_owned(),
            resources: self.resources,
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
            log_directory: "/var/log/pods/".to_owned() + &self.name,
            port_mappings: vec![],
            annotations: HashMap::new(),
            windows: None,
        };
        config
    }
}

pub async fn pull_image(isc: &mut ImageServiceClient<Channel>, name: String) -> Result<cri::PullImageResponse, Status> {
    isc.pull_image(cri::PullImageRequest{
        image: Some(cri:: ImageSpec {
            image: name,
            annotations: Default::default(),
            ..Default::default()
        }),
        auth: None,
        sandbox_config: None,
    })
        .await
        .map(|m| m.into_inner())
}

pub async fn create_sandbox(mut rsc: RuntimeServiceClient<Channel>, config: SandBoxConfig) -> Result<(String, cri::PodSandboxConfig), Status> {
    let config = config.to_cri_config();
    let request = cri::RunPodSandboxRequest {
        config: Some(config.clone()),
        runtime_handler: String::new(),
    };
    return rsc.run_pod_sandbox(request)
        .await
        .map(|m| (m.into_inner().pod_sandbox_id, config));
}

pub async fn create_container(mut rsc: RuntimeServiceClient<Channel>, pod_id: String, config: ContainerConfig, sandbox_config: SandBoxConfig)
    -> Result<String, Status>
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
            ..Default::default()
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
        cdi_devices: vec![],
    };
    let create_request = cri::CreateContainerRequest {
        pod_sandbox_id: pod_id,
        config: Some(cri_container_config),
        sandbox_config: Some(sandbox_config.to_cri_config()),
    };
    
    rsc.create_container(create_request)
        .await
        .map(|m| m.into_inner().container_id)
}

pub async fn start_container(mut rsc: RuntimeServiceClient<Channel>, id: String) -> Result<(), Status> {
    rsc.start_container(cri::StartContainerRequest { container_id: id })
        .await
        .map(|_| ())
}

pub async fn stop_container(mut rsc: RuntimeServiceClient<Channel>, container_id: String) -> Result<(), Status> {
    let stop_req = cri::StopContainerRequest {
        container_id,
        timeout: 0,
    };
    rsc.stop_container(stop_req)
        .await
        .map(|_| ())
}

pub async fn remove_container(mut rsc: RuntimeServiceClient<Channel>, container_id: String) -> Result<(), Status> {
    let remove_req = cri::RemoveContainerRequest {
        container_id: container_id,
    };
    rsc.remove_container(remove_req).await.map(|_| ())
}

pub async fn remove_pod(mut rsc: RuntimeServiceClient<Channel>, pod_id: String) -> Result<(), Status> {
    let stop_req = cri::StopPodSandboxRequest {
        pod_sandbox_id: pod_id.clone()
    };
    let _stop_resp = rsc.stop_pod_sandbox(stop_req)
        .await
        .map(|m| m.into_inner())?;

    let remove_req = cri::RemovePodSandboxRequest {
        pod_sandbox_id: pod_id.clone()
    };
    let remove_resp = rsc.remove_pod_sandbox(remove_req)
        .await
        .map(|_| ());

    return remove_resp;
}

pub async fn list_containers(mut rsc: RuntimeServiceClient<Channel>) -> Result<cri::ListContainersResponse, Status> {
    let list_req = cri::ListContainersRequest {
        filter: None
    };
    rsc.list_containers(list_req)
        .await
        .map(|m| m.into_inner())
}

pub async fn list_pods(mut rsc: RuntimeServiceClient<Channel>) -> Result<cri::ListPodSandboxResponse, Status> {
    let list_req = cri::ListPodSandboxRequest {
        filter: None,
    };
    rsc.list_pod_sandbox(list_req)
        .await
        .map(|m| m.into_inner())
}
