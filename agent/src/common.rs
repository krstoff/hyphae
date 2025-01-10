pub use k8s_cri::v1 as cri;
pub use std::collections::HashMap;
pub use std::time::Duration;
pub use cri::image_service_client::ImageServiceClient;
pub use cri::runtime_service_client::RuntimeServiceClient;

pub type RuntimeService = RuntimeServiceClient<tonic::transport::Channel>;

pub use crate::operations::{SandBoxConfig, ContainerConfig};

pub type UID = String;
pub type PodId = String;
pub type Name = String;
pub type CtrId = String;
pub type StateHandle = std::sync::Arc<tokio::sync::Mutex<crate::state::State>>;

pub fn log_err<E: std::error::Error>(e: E) {

}