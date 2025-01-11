use crate::*;

#[tokio::test]
async fn setup_teardown() {
    let channel = connect_uds().await.expect("Could not connect to containerd");
    let mut image_service = ImageService::new(channel.clone());
    let image_name = "docker.io/library/nginx:latest".to_owned();
    let pull_image_response = operations::pull_image(&mut image_service, image_name.clone()).await.unwrap();
    println!("{:?}", &pull_image_response);

    fn make_uid() -> String {
        return "123456789".to_owned();
    }
    let uid = make_uid();
    let mut rsc = RuntimeService::new(channel);
    let sandbox_config = SandBoxConfig {
        name: "nginx".to_owned(),
        uid: uid.clone(),
        namespace: "default".to_owned(),
        resources: None,
    };
    let pod_id = operations::create_sandbox(rsc.clone(), sandbox_config.clone()).await.unwrap();
    println!("Created sandbox: {:?}", &pod_id);

    let container_config = ContainerConfig {
        name: "nginx-container".to_owned(),
        image: pull_image_response.image_ref.clone(),
        command: "nginx".to_owned(),
        args: vec![],
        working_dir: "".to_owned(),
        envs: vec![],
        privileged: false
    };
    let cid = operations::create_container(rsc.clone(), pod_id.clone(), container_config, sandbox_config).await.unwrap();
    println!("Created container: {}", &cid);
    operations::start_container(rsc.clone(), cid).await.unwrap();
    let remove_response = operations::remove_pod(rsc.clone(), pod_id.clone()).await.unwrap();
    println!("{:?}", &remove_response);
}