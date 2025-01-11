pub use k8s_cri::v1 as cri;
pub use std::collections::HashMap;
pub use std::time::Duration;
use cri::runtime_service_client::RuntimeServiceClient;
use cri::image_service_client::ImageServiceClient;
pub use tokio::select;

pub type RuntimeService = RuntimeServiceClient<tonic::transport::Channel>;
pub type ImageService = ImageServiceClient<tonic::transport::Channel>;

pub use crate::operations::{SandBoxConfig, ContainerConfig};

pub type UID = String;
pub type PodId = String;
pub type Name = String;
pub type CtrId = String;

#[derive(Debug, Clone)]
pub enum Error {
    CriError(tonic::Status)
}

impl std::fmt::Display for Error{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for Error {

}

impl From<tonic::Status> for Error {
    fn from(status: tonic::Status) -> Error {
        Error::CriError(status)
    }
}

pub fn log_err<E: std::error::Error>(e: E) {
    println!("{}", e.to_string());
}