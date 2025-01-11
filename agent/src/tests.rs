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
    rsc.start_container(cid).await.unwrap();
    let _ = rsc.remove_pod(pod_id.clone()).await.unwrap();
}

fn make_alpine_config(name: &str) -> ContainerConfig {
    let container_config = ContainerConfig {
        name: name.to_owned(),
        image: "docker.io/library/alpine:latest".to_owned(),
        command: "/bin/sh".to_owned(),
        args: vec!["-c".to_owned(), "while true; do sleep 1; done".to_owned()],
        working_dir: "".to_owned(),
        envs: vec![],
        privileged: false
    };
    container_config
}

#[tokio::test]
async fn test_agent() {
    async fn poll_for_target(mut target_tx: WatchTx<state::Target>) -> Result<(), Error> {
        let config = SandBoxConfig {
            name: "testpod".to_owned(),
            uid: "123456789".to_owned(),
            namespace: "default".to_owned(),
            resources: None,
        };
        let mut containers = HashMap::new();
        for i in 0..10 {
            let name = format!("alpine{}", i);
            containers.insert(name.clone(), make_alpine_config(&name));
        }
        let mut target = state::Target::new();
        target.pods.insert(config.uid.clone(), PodConfig {
            config, containers
        });
        
        target_tx.send(target).unwrap();
        loop {
            tokio::time::sleep(Duration::from_millis(1_000)).await;
        }
    }
    let runtime = RuntimeClient::connect().await.expect("Could not connect to containerd.");
    let (events_tx, events_rx) = tokio::sync::mpsc::channel(EVENTS_BUFFER_MAX);
    let (target_tx, target_rx) = tokio::sync::watch::channel(state::Target::new());

    let target_process = tokio::spawn(poll_for_target(target_tx));
    let events_process = tokio::spawn(read_events(runtime.clone(), events_tx));
    let control_process = tokio::spawn(control_loop(runtime.clone(), events_rx, target_rx));

    select! {
        result = target_process => {
            match result {
                Err(e) => log_err(e),
                Ok(_) => (),
            }
        }
        result = events_process => {
            match result {
                Err(e) => log_err(e),
                Ok(_) => (),
            }
        }
        result = control_process => {
            match result {
                Err(e) => log_err(e),
                Ok(_) => (),
            }
        }
    }
}