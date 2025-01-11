use crate::*;

#[tokio::test]
async fn setup_teardown() {
    fn make_uid() -> String {
        return "123456789".to_owned();
    }
    let mut rsc = RuntimeClient::connect().await.unwrap();

    let uid = make_uid();
    let sandbox_config = SandBoxConfig {
        name: "testpod".to_owned(),
        uid: uid.clone(),
        namespace: "default".to_owned(),
        resources: None,
    };
    let pod_id = rsc.create_sandbox(sandbox_config.clone()).await.unwrap();
    println!("Created sandbox: {:?}", &pod_id);

    let container_config = ContainerConfig {
        name: "test_container".to_owned(),
        image: "docker.io/library/debian:latest".to_owned(),
        command: "/bin/sh".to_owned(),
        args: vec!["-c".to_owned(), "while true; do sleep 1; done".to_owned()],
        working_dir: "".to_owned(),
        envs: vec![],
        privileged: false
    };
    let _ = rsc.pull_image(container_config.image.clone()).await.unwrap();
    let cid = rsc.create_container(pod_id.clone(), container_config, sandbox_config).await.unwrap();
    println!("Created container: {}", &cid);
    rsc.start_container(cid).await.unwrap();
    let remove_response = rsc.remove_pod(pod_id.clone()).await.unwrap();
    println!("{:?}", &remove_response);
}