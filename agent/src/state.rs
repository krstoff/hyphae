use crate::common::*;

pub fn to_state(i: i32) -> cri::ContainerState {
    i.try_into().unwrap()
}

#[derive(Clone)]
pub struct CtrStatus {
    id: CtrId,
    state: cri::ContainerState,
}

#[derive(Clone)]
pub struct PodStatus {
    id: PodId,
    ctrs: HashMap<Name, CtrStatus>,
}

/// The current state of the node.
pub struct State {
    pub pods: HashMap<UID, PodStatus>
}

impl State {
    pub fn new() -> StateHandle {
        use std::sync::Arc;
        use tokio::sync::Mutex;
        Arc::new(Mutex::new(State { pods: HashMap::new() }))
    }

    pub fn process_message(&mut self, message: cri::ContainerEventResponse) {
        let id = message.container_id;
        if message.pod_sandbox_status.is_none() { // Pod Deletion Event
            self.pods.retain(|_, podstatus| { podstatus.id != id });
            return;
        }
        let sandbox = message.pod_sandbox_status.unwrap();
        if &id == &sandbox.id { // Pod Creation event
            self.pods.insert(
                sandbox.metadata.unwrap().uid.clone(), 
                PodStatus { id: id.clone(), ctrs: HashMap::new() }
            );
            return;
        }

        // Just replace the whole pod state. The message contains everything.
        let mut ctrs = HashMap::new();
        for container in message.containers_statuses {
            ctrs.insert(
                container.metadata.unwrap().name,
                CtrStatus { id: container.id, state: to_state(container.state) }
            );
        }
        let pod = PodStatus { id: sandbox.id.clone(), ctrs };
        self.pods.entry(sandbox.metadata.unwrap().uid.clone())
            .and_modify(|p| *p = pod.clone())
            .or_insert(pod);
    }

    pub fn ingest(&mut self, containers: Vec<cri::Container>, pods: Vec<cri::PodSandbox>) {
        todo!();
        self.pods.clear();
    }
}

#[derive(Clone)]
pub struct PodConfig {
    pub config: SandBoxConfig,
    pub containers: HashMap<Name, ContainerConfig>,
}

/// The intended state of the node.
pub struct Target {
    pub pods: HashMap<UID, PodConfig>
}

impl Target {
    pub fn new() -> Target {
        Target { pods: HashMap::new() }
    }
}

pub enum PodStep {
    CreatePod(SandBoxConfig),
    ChangePod(HashMap<Name, ContainerStep>),
    DeletePod(PodId),
}

pub enum ContainerStep {
    CreateCtr(PodId, ContainerConfig, SandBoxConfig),
    StartCtr(CtrId),
    StopCtr(CtrId),
    DeleteCtr(CtrId),
}

/// A tree of steps that will get us from State to Target
pub struct Plan {
    pub pods: HashMap<UID, PodStep>
}
fn diff(target: Target, state: State) -> Plan {
    use PodStep::*;
    use ContainerStep::*;
    use cri::ContainerState as CS;
    let mut plan = Plan { pods: HashMap::new() };

    // Check that every pod in target exists in state
    for (uid, podconfig) in target.pods.iter() {
        if !state.pods.contains_key(uid) {
            plan.pods.insert(
                uid.clone(),
                CreatePod (podconfig.config.clone())
            );
            continue;
        }
        let pod = state.pods.get(uid).unwrap();
        let mut steps = HashMap::new();
        // Check that every pod's container exists and is running
        // TODO: This assumes that every container's desired state is RUNNING. Eventually we will support Jobs, whose desired state is EXITED with status code 0.
        for (name, ctrconfig) in podconfig.containers.iter() {
            let step = match pod.ctrs.get(name) {
                None => CreateCtr(pod.id.clone(), ctrconfig.clone(), podconfig.config.clone()),
                Some(&CtrStatus{ ref id, state: CS::ContainerCreated }) => StartCtr(id.clone()),
                Some(&CtrStatus{ state: CS::ContainerRunning, .. }) => { continue; }
                Some(&CtrStatus{ ref id, state: CS::ContainerExited }) => DeleteCtr(id.clone()),
                Some(&CtrStatus{ state: CS::ContainerUnknown, .. }) => { continue; }
            };
            steps.insert(name.clone(), step);
        }
        if steps.len() > 0 {
            plan.pods.insert(
                uid.clone(),
                ChangePod(steps)
            );
        }
    }

    // Check that every pod that is running is meant to be
    for (uid, podstatus) in state.pods.iter() {
        if !target.pods.contains_key(uid) {
            // Check that every container is stopped first.
            let mut steps = HashMap::new();
            for (name, ctrstatus) in podstatus.ctrs.iter() {
                let step = match ctrstatus {
                    &CtrStatus { ref id, state: CS::ContainerRunning } => StopCtr(id.clone()),
                    _ => { continue; }
                };
                steps.insert(name.clone(), step);
            }
            if steps.len() > 0 {
                plan.pods.insert(uid.clone(), ChangePod(steps));
            } else {
                plan.pods.insert(uid.clone(), DeletePod(podstatus.id.clone()));
            }
            continue;
        }
        let target_pod = target.pods.get(uid).unwrap();
        // Pod exists, but we have to be sure we're not running extra containers.
        let mut steps = HashMap::new();
        for (name, ctrstatus) in podstatus.ctrs.iter() {
            if !target_pod.containers.contains_key(name) {
                let step = match ctrstatus {
                    &CtrStatus { ref id, state: CS::ContainerCreated } => DeleteCtr(id.clone()),
                    &CtrStatus { ref id, state: CS::ContainerRunning } => StopCtr(id.clone()),
                    &CtrStatus { ref id, state: CS::ContainerExited } => DeleteCtr(id.clone()),
                    &CtrStatus { ref id, state: CS::ContainerUnknown } => { continue; }
                };
                steps.insert(name.clone(), step);
            }
        }
        if steps.len() > 0 {
            match plan.pods.get_mut(uid) {
                None => {
                    plan.pods.insert(uid.clone(), ChangePod(steps));
                }
                Some(&mut ChangePod(ref mut first_steps)) => {
                    for (name, step) in steps {
                        first_steps.insert(name, step);
                    }
                }
                _ => unreachable!("Tried to add or delete containers to a pod marked for deletion.")
            }
        }
    }

    plan
}

// impl std::fmt::Debug for State {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
//         f.write_str("pods: ")?;
//         f.debug_map().entries(self.pods.iter()).finish()?;
//         f.write_str("\n")?;
//         Ok(())
//     }
// }

#[cfg(test)]
mod tests {

}