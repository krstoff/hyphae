use k8s_cri::v1::runtime_service_client::RuntimeServiceClient;
use k8s_cri::v1::ContainerState as CS;
use tokio::sync::Mutex;
use tonic::Status;
use std::sync::Arc;
use std::collections::HashMap;
use std::time::Duration;

use crate::operations as ops;
use crate::state::State;
use crate::StateHandle;
use crate::operations::{ContainerConfig, SandBoxConfig};

pub const CONTROL_LOOP_INTERVAL_MS: u64 = 3_000;
type UID = String;
type Name = String;

type RuntimeService = RuntimeServiceClient<tonic::transport::Channel>;

#[derive(Clone)]
pub struct PodConfig {
    pub config: SandBoxConfig,
    pub containers: HashMap<Name, ContainerConfig>,
}
pub struct Target {
    spec: HashMap<UID, PodConfig>
}
type TargetHandle = Arc<Mutex<Target>>;

impl Target {
    pub fn new() -> TargetHandle {
        Arc::new(Mutex::new(Target { spec: HashMap::new() }))
    }
}

enum Action {
    StartPod(UID, SandBoxConfig),
    CreateContainer((UID, Name), ContainerConfig, SandBoxConfig),
    StartContainer((UID, Name)),
    StopContainer((UID, Name)),
    DeleteContainer((UID, Name)),
    DeletePod(UID)
}

// A task is a set of steps that have sequential dependency.
struct Task {
    steps: Vec<Action>
}

impl Task {
    pub fn start_pod(uid: String, pod: PodConfig) -> Task {
        let sandbox_config = pod.config;
        let containers = pod.containers;
        let mut steps = vec![];

        steps.push(Action::StartPod(uid.clone(), sandbox_config.clone()));

        for (name, container_config) in containers {
            steps.push(Action::CreateContainer((uid.clone(), name.clone()), container_config, sandbox_config.clone()));
            steps.push(Action::StartContainer((uid.clone(), name)));
        }
        Task { steps }
    }
    pub fn run_container(key: (UID, Name), config: ContainerConfig, sandbox_config: SandBoxConfig) -> Task {
        let mut steps = vec![];
        steps.push(Action::CreateContainer(key.clone(), config, sandbox_config));
        steps.push(Action::StartContainer(key));
        Task { steps }
    }
    pub fn restart_container(key: (UID, Name), config: ContainerConfig, sandbox_config: SandBoxConfig) -> Task {
        let mut steps = vec![];
        steps.push(Action::DeleteContainer(key.clone()));
        steps.push(Action::CreateContainer(key.clone(), config, sandbox_config));
        steps.push(Action::StartContainer(key));
        Task { steps }
    }
    pub fn start_container(key: (UID, Name)) -> Task {
        let mut steps = vec![];
        steps.push(Action::StartContainer(key));
        Task { steps }
    }
    pub fn delete_container(key: (UID, Name)) -> Task {
        Task { steps: vec![Action::DeleteContainer(key)]}
    }

    pub async fn execute(self, rsc: &mut RuntimeService, state: &mut State) -> Result<(), Status> {
        for step in self.steps {
            match step {
                Action::StartPod(uid, sandbox_config) => {
                    let (pod_id, pod_config) = ops::create_sandbox(rsc, sandbox_config).await?;
                    state.uids.insert(uid, pod_id);
                }
                Action::CreateContainer((uid, name), container_config, sandbox_config) => {
                    let pod_id = state.uids.get(&uid).unwrap();
                    let ctr_id = ops::create_container(rsc, pod_id.clone(), container_config, sandbox_config).await?;
                    state.names.insert((uid, name), ctr_id);
                }
                Action::StartContainer(key) => {
                    let cid = state.names.get(&key).unwrap();
                    ops::start_container(rsc, cid.clone()).await?;
                }
                Action::StopContainer(key) => {
                    let cid = state.names.get(&key).unwrap();
                    ops::stop_container(rsc, cid.clone()).await?;
                }
                Action::DeleteContainer(key) => {
                    let cid = state.names.get(&key).unwrap();
                    ops::remove_container(rsc, cid.clone()).await?;
                    state.names.remove(&key);
                }
                Action::DeletePod(uid) => {
                    let pid = state.uids.get(&uid).unwrap();
                    ops::remove_pod(rsc, pid.clone()).await?;
                    state.uids.remove(&uid);
                    state.names.retain(|&(ref uid2, _), _| uid2 != &uid);
                }
            }
        }
        Ok(())
    }
}

pub async fn control_loop(
    mut runtime_service: RuntimeServiceClient<tonic::transport::Channel>, 
    mut runtime_state: StateHandle,
    mut target_state: TargetHandle
) {
    loop {
        let mut state = runtime_state.lock().await;
        let target = target_state.lock().await;
        let plan = reconcile(&mut state, &target);
        let results = execute(&mut runtime_service, &mut state, plan).await;
        tokio::time::sleep(Duration::from_millis(CONTROL_LOOP_INTERVAL_MS)).await;
    }
}

fn reconcile<'a>(state: &mut State, target: &Target) -> Vec<Task> {
    let mut plan: Vec<Task> = vec![];

    // Check that all UIDs in target are present in state
    for (uid, podconfig) in target.spec.iter() {
        if !state.uids.contains_key(uid) {
            plan.push(Task::start_pod(uid.clone(), podconfig.clone()));
        }
        
        if !state.pods.contains_key(state.uids.get(uid).unwrap()) { // pod was deleted
            state.uids.remove(uid);
            plan.push(Task::start_pod(uid.clone(), podconfig.clone()));
        }

        // check that every name in the pod is present
        for (name, ctrconfig) in podconfig.containers.iter() {
            let key = (uid.clone(), name.clone());
            if !state.names.contains_key(&key) {
                plan.push(Task::run_container(key.clone(), ctrconfig.clone(), podconfig.config.clone()));
            } else {
                let cid = state.names.get(&key).unwrap();
                if !state.ctrs.contains_key(cid) {
                    state.names.remove(&key);
                    plan.push(Task::run_container(key.clone(), ctrconfig.clone(), podconfig.config.clone()));
                    continue;
                }
                match state.ctrs.get(cid).unwrap() {
                    &CS::ContainerRunning => {} // good
                    &CS::ContainerExited => { // bad
                        plan.push(Task::restart_container(key.clone(), ctrconfig.clone(), podconfig.config.clone()));
                    }
                    &CS::ContainerCreated => {
                        plan.push(Task::start_container(key.clone()));
                    }
                    &CS::ContainerUnknown => {} //?
                }
            }            
        }
    }

    // check that every named container in the state is supposed to be running
    let mut dangling_names = vec![]; // We leave this outside the for-loop to keep the borrow-checker happy
    let mut ctrs_to_delete = HashMap::new(); // We collect these while iterating over the containers so that we can group them with the pod at the end.
    for (key @ &(ref uid, ref name), cid) in state.names.iter() {
        let mut pod_ok = target.spec.contains_key(uid);
        if pod_ok && target.spec.get(uid).unwrap().containers.contains_key(name) {
                continue;
        }
        if !state.ctrs.contains_key(cid) {
            dangling_names.push(key.clone());
            continue;
        }
        if pod_ok {
            plan.push(Task::delete_container(key.clone()));
            continue;
        }

        match state.ctrs.get(cid).unwrap() {
            &CS::ContainerRunning => {
                ctrs_to_delete.entry(uid.clone())
                    .and_modify(|steps: &mut Vec<Action>| steps.push(Action::StopContainer(key.clone())))
                    .or_insert(vec![Action::StopContainer(key.clone())]);
            },
            _ => {}
        }

    }
    for key in dangling_names { state.names.remove(&key); } 

    // check that every uid in state is supposed to be running
    let mut dangling_names = vec![];
    for (uid, pid) in state.uids.iter() {
        if !state.pods.contains_key(pid) {
            dangling_names.push(uid.clone());
            continue;
        }
        if !target.spec.contains_key(uid) {
            ctrs_to_delete.entry(uid.clone())
                .and_modify(|steps| steps.push(Action::DeletePod(uid.clone())))
                .or_insert(vec![Action::DeletePod(uid.clone())]);
        }
    }
    for uid in dangling_names { state.uids.remove(&uid); }
    for (_, steps) in ctrs_to_delete {
        plan.push(Task { steps });
    }
    return plan;
}

async fn execute(rsc: &mut RuntimeService, state: &mut State, plan: Vec<Task>) {
    for task in plan {
        let result = task.execute(rsc, state);
    }
}

#[cfg(test)]
mod tests {

}
