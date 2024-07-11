use containerd_client::connect;
use containerd_client::services::v1::{
    namespaces_client::NamespacesClient,
    images_client::ImagesClient,
    transfer_client::TransferClient,
    CreateNamespaceRequest,
    Namespace,
    TransferRequest,
};
use containerd_client::types::v1::transfer::UnpackConfiguration;
use containerd_client::types::v1::{
    transfer::ImageStore,
    transfer::OciRegistry,
};
use containerd_client::types::{
    Platform,
};
use containerd_client::with_namespace;
use containerd_client::tonic;
use tonic::Request;

static IMAGE_REFERENCE: &'static str = "docker.io/library/nginx:latest";

async fn initialize_namespace(channel: &mut tonic::transport::Channel) {
    let mut client = NamespacesClient::new(channel);
    let resp = client.create(CreateNamespaceRequest {
        namespace: Some(Namespace { name: "hyphae-agent-test".to_owned(), labels: Default::default()})
    }).await;
    match resp {
        Ok(_) => println!("Created namespace."),
        Err(_) => println!("Namespace already existed.")
    }
}

async fn pull_image(channel: &mut tonic::transport::Channel) {
    let registry = OciRegistry {
        reference: IMAGE_REFERENCE.to_owned(),
        resolver: None,
    };
    let image_store = ImageStore {
        name: "nginx".to_owned(),
        labels: Default::default(),
        platforms: vec![
            Platform {
                architecture: "amd64".to_owned(),
                os: "linux".to_owned(),
                variant: "".to_owned(),
            }
        ],
        all_metadata: false,
        manifest_limit: 0,
        extra_references: Default::default(),
        unpacks: vec!(UnpackConfiguration {
            platform: Some(Platform {
                architecture: "amd64".to_owned(),
                os: "linux".to_owned(),
                variant: "".to_owned(),
            }),
            snapshotter: "".to_owned(),
        }),
    };
    let mut client = TransferClient::new(channel);
    let mut src = prost_types::Any::from_msg(&registry).unwrap();
    let mut dst = prost_types::Any::from_msg(&image_store).unwrap();
    // containerd not registering compliant type-urls, have to trim leading slash...
    src.type_url = src.type_url[1..].to_owned();
    dst.type_url = dst.type_url[1..].to_owned();
    let req = TransferRequest {
        source: Some(src),
        destination: Some(dst),
        options: None
    };
    let req = with_namespace!(req, "default");
    let resp = client.transfer(req).await.unwrap();
}

#[tokio::main]
async fn main() {
    // Launch containerd at /run/containerd/containerd.sock
    let mut channel = connect("/run/containerd/containerd.sock").await.unwrap();
    initialize_namespace(&mut channel).await;
    pull_image(&mut channel).await;
}
