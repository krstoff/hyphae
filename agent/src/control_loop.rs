use k8s_cri::v1::runtime_service_client::RuntimeServiceClient;
use k8s_cri::v1::ContainerState as CS;
use tokio::sync::Mutex;
use std::sync::Arc;
use std::collections::HashMap;
use std::time::Duration;

use crate::operations::*;
use crate::state::State;
use crate::StateHandle;
use crate::operations::{ContainerConfig, SandBoxConfig};

pub const CONTROL_LOOP_INTERVAL_MS: u64 = 3_000;
type UID = String;
type Name = String;
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

enum Action<'a> {
    StartPod(&'a UID, &'a PodConfig),
    CreateContainer((UID, Name), &'a ContainerConfig),
    StartContainer((UID, Name), &'a ContainerConfig),
    RestartContainer((UID, Name), &'a ContainerConfig),
    StopContainer((UID, Name)),
    DeleteContainer((UID, Name)),
    DeletePod(UID)
}

pub async fn control_loop(
    mut runtime_service: RuntimeServiceClient<tonic::transport::Channel>, 
    mut runtime_state: StateHandle,
    mut target_state: TargetHandle
) {
    loop {
        let mut state = runtime_state.lock().await;
        let target = target_state.lock().await;
        let _plan = reconcile(&mut state, &target);
        tokio::time::sleep(Duration::from_millis(CONTROL_LOOP_INTERVAL_MS)).await;
    }
}

fn reconcile<'a>(state: &mut State, target: &'a Target) -> Vec<Action<'a>> {
    let mut plan: Vec<Action> = vec![];

    // Check that all UIDs in target are present in state
    for (uid, podconfig) in target.spec.iter() {
        if !state.uids.contains_key(uid) {
            plan.push(Action::StartPod(uid, podconfig))
        }
        
        if !state.pods.contains_key(state.uids.get(uid).unwrap()) { // pod was deleted
            state.uids.remove(uid);
            plan.push(Action::StartPod(uid, podconfig));
        }

        // check that every name in the pod is present
        for (name, ctrconfig) in podconfig.containers.iter() {
            let key = (uid.clone(), name.clone());
            if !state.names.contains_key(&key) {
                plan.push(Action::CreateContainer(key.clone(), ctrconfig));
                plan.push(Action::StartContainer(key.clone(), ctrconfig));
            } else {
                let cid = state.uids.get(uid).unwrap();
                if !state.ctrs.contains_key(cid) {
                    state.names.remove(&key);
                    plan.push(Action::CreateContainer(key.clone(), ctrconfig));
                    plan.push(Action::StartContainer(key.clone(), ctrconfig));
                    continue;
                }
                match state.ctrs.get(cid).unwrap() {
                    &CS::ContainerRunning => {} // good
                    &CS::ContainerExited => { // bad
                        plan.push(Action::RestartContainer(key.clone(), ctrconfig));
                    }
                    &CS::ContainerCreated => {
                        plan.push(Action::StartContainer(key.clone(), ctrconfig));
                    }
                    &CS::ContainerUnknown => {} //?
                }
            }            
        }
    }

    // check that every named container in the state is supposed to be running
    let mut dangling_names = vec![]; // We leave this outside the for-loop to keep the borrow-checker happy
    for (key @ &(ref uid, ref name), cid) in state.names.iter() {
        let mut pod_ok = target.spec.contains_key(uid);
        if pod_ok && target.spec.get(uid).unwrap().containers.contains_key(name) {
                continue;
        }
        if !state.ctrs.contains_key(cid) {
            dangling_names.push(key.clone());
            continue;
        }
        match state.ctrs.get(cid).unwrap() {
            &CS::ContainerRunning => {
                plan.push(Action::StopContainer(key.clone()));
            }
            _ => {}
        }
        if pod_ok {
            plan.push(Action::DeleteContainer(key.clone()))
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
            plan.push(Action::DeletePod(uid.clone()));
        }
    }
    for uid in dangling_names { state.uids.remove(&uid); }
    return plan;
}