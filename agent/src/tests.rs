use crate::*;

#[tokio::test]
pub async fn setup_teardown() {
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
    let run_container_response = run_container(&mut runtime_service, container_config, podsandbox_config).await;
    println!("{:?}", &run_container_response);

    tokio::time::sleep(std::time::Duration::from_millis(5000)).await;
    let remove_response = remove_pod(&mut &mut runtime_service, create_sandbox_response.pod_sandbox_id).await;
    println!("{:?}", &remove_response);
}