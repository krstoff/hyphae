use cri::image_service_client::ImageServiceClient;
use cri::runtime_service_client::RuntimeServiceClient;
use k8s_cri::v1 as cri;
use tonic::Status;
use std::collections::HashMap;

type RuntimeService = RuntimeServiceClient<tonic::transport::Channel>;
type ImageService = ImageServiceClient<tonic::transport::Channel>;

#[derive(Clone)]
pub struct SandBoxConfig {
    pub name: String,
    pub uid: String,
    pub resources: Option<cri::LinuxContainerResources>,
    pub namespace: String,
}

#[derive(Clone)]
pub struct ContainerConfig {
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

#[derive(Clone)]
pub struct RuntimeClient {
    rsc: RuntimeService,
    isc: ImageService,
}

impl RuntimeClient {
    pub async fn connect() -> Result<Self, tonic::transport::Error> {
        use hyper_util::rt::TokioIo;
        use tokio::net::UnixStream;
        let channel = tonic::transport::Endpoint::try_from("http://[::]:50051")?
        .connect_with_connector(
            tower::service_fn(move |_| async {
                Ok::<_, std::io::Error>(TokioIo::new(UnixStream::connect(crate::CONTAINERD_SOCKET_PATH).await?))
            })
        ).await?;
        let rsc = RuntimeService::new(channel.clone());
        let isc = ImageService::new(channel.clone());
        Ok(RuntimeClient { rsc, isc })
    }
    pub async fn pull_image(&mut self, name: String) -> Result<cri::PullImageResponse, Status> {
        self.isc.pull_image(cri::PullImageRequest{
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
    
    pub async fn create_sandbox(&mut self, config: SandBoxConfig) -> Result<String, Status> {
        let config = config.to_cri_config();
        let request = cri::RunPodSandboxRequest {
            config: Some(config.clone()),
            runtime_handler: String::new(),
        };
        return self.rsc.run_pod_sandbox(request)
            .await
            .map(|m| m.into_inner().pod_sandbox_id);
    }
    
    pub async fn create_container(&mut self, pod_id: String, config: ContainerConfig, sandbox_config: SandBoxConfig)
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
            metadata: Some(cri::ContainerMetadata {
                name: config.name.clone(),
                attempt: 0,
            }),
            image: Some(cri::ImageSpec {
                image: config.image.clone(),
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
        
        self.rsc.create_container(create_request)
            .await
            .map(|m| m.into_inner().container_id)
    }
    
    pub async fn start_container(&mut self, id: String) -> Result<(), Status> {
        self.rsc.start_container(cri::StartContainerRequest { container_id: id })
            .await
            .map(|_| ())
    }
    
    pub async fn stop_container(&mut self, container_id: String) -> Result<(), Status> {
        let stop_req = cri::StopContainerRequest {
            container_id,
            timeout: 0,
        };
        self.rsc.stop_container(stop_req)
            .await
            .map(|_| ())
    }
    
    pub async fn remove_container(&mut self, container_id: String) -> Result<(), Status> {
        let remove_req = cri::RemoveContainerRequest {
            container_id: container_id,
        };
        self.rsc.remove_container(remove_req).await.map(|_| ())
    }
    
    pub async fn remove_pod(&mut self, pod_id: String) -> Result<(), Status> {
        let stop_req = cri::StopPodSandboxRequest {
            pod_sandbox_id: pod_id.clone()
        };
        let _stop_resp = self.rsc.stop_pod_sandbox(stop_req)
            .await
            .map(|m| m.into_inner())?;
    
        let remove_req = cri::RemovePodSandboxRequest {
            pod_sandbox_id: pod_id.clone()
        };
        let remove_resp = self.rsc.remove_pod_sandbox(remove_req)
            .await
            .map(|_| ());
    
        return remove_resp;
    }
    
    pub async fn list_containers(&mut self) -> Result<cri::ListContainersResponse, Status> {
        let list_req = cri::ListContainersRequest {
            filter: None
        };
        self.rsc.list_containers(list_req)
            .await
            .map(|m| m.into_inner())
    }
    
    pub async fn list_pods(&mut self) -> Result<cri::ListPodSandboxResponse, Status> {
        let list_req = cri::ListPodSandboxRequest {
            filter: None,
        };
        self.rsc.list_pod_sandbox(list_req)
            .await
            .map(|m| m.into_inner())
    }

    pub async fn get_container_events(&mut self) -> Result<tonic::Streaming<cri::ContainerEventResponse>, tonic::Status> {
        self.rsc.get_container_events(cri::GetEventsRequest{}).await
            .map(|stream| stream.into_inner())
    }
}