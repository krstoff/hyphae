use crate::common::*;

pub fn to_state(i: i32) -> cri::ContainerState {
    i.try_into().unwrap()
}

#[derive(Clone, Debug)]
pub struct CtrStatus {
    id: CtrId,
    state: cri::ContainerState,
}

#[derive(Clone, Debug)]
pub struct PodStatus {
    id: PodId,
    ctrs: HashMap<Name, CtrStatus>,
}

/// The current state of the node.
pub struct State {
    pub pods: HashMap<UID, PodStatus>
}

impl State {
    pub fn new() -> State {
        State { pods: HashMap::new() }
    }

    pub fn observe(&mut self, message: cri::ContainerEventResponse) {
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
        self.pods.clear();
        let mut uids = HashMap::new(); // id -> uid
        for pod in pods {
            let uid = pod.metadata.unwrap().uid;
            uids.insert(pod.id.clone(), uid.clone());
            self.pods.insert(uid, PodStatus { id: pod.id.clone(), ctrs: HashMap::new() });
        }
        for ctr in containers {
            let name = ctr.labels.get("name").unwrap().clone();
            let pod_uid = uids.get(&ctr.pod_sandbox_id).unwrap();
            let pod = self.pods.get_mut(pod_uid).unwrap();
            pod.ctrs.insert(name, CtrStatus {
                id: ctr.id,
                state: to_state(ctr.state),
            });
        }
    }
}

/// The intended state of the node.
#[derive(Clone)]
pub struct Target {
    pub pods: HashMap<UID, PodConfig>
}

impl Target {
    pub fn new() -> Target {
        Target { pods: HashMap::new() }
    }
}

#[derive(Clone, Debug)]
pub enum PodStep {
    CreatePod(SandBoxConfig),
    ChangePod(HashMap<Name, ContainerStep>),
    DeletePod(PodId),
}

#[derive(Clone, Debug)]
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

pub fn diff(target: &Target, state: &State) -> Plan {
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
                    &CtrStatus { id: _, state: CS::ContainerUnknown } => { continue; }
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

impl std::fmt::Debug for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        writeln!(f, "Target: {{")?;
        for (uid, pod) in self.pods.iter() {
            writeln!(f, "    {}: {{", uid)?;
            writeln!(f, "        <name>: {}", pod.config.name)?;
            for (name, ctr) in pod.containers.iter() {
                writeln!(f, "        {}: {}", name, ctr.image)?;
            }
            writeln!(f, "    }}")?;
        }
        writeln!(f, "}}")?;
        Ok(())
    }
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        writeln!(f, "State: {{")?;
        for (uid, pod) in self.pods.iter() {
            writeln!(f, "    {}: {{", uid)?;
            writeln!(f, "        <id>: {}", pod.id)?;
            for (name, ctr) in pod.ctrs.iter() {
                writeln!(f, "        {}: ({}, {:?})", name, ctr.id, ctr.state)?;
            }
            writeln!(f, "    }}")?;
        }
        writeln!(f, "}}")?;
        Ok(())
    }
}

impl std::fmt::Debug for Plan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        use PodStep::*;
        use ContainerStep::*;
        writeln!(f, "Plan: {{")?;
        for (uid, pod) in self.pods.iter() {
            write!(f, "    {}:", uid)?;
            match pod {
                &CreatePod(ref config) => {
                    writeln!(f, "CREATEPOD {}", &config.name)?;
                }
                &ChangePod(ref ctrs) => {
                    writeln!(f, "CHANGEPOD {{")?;
                    for (name, ctr) in ctrs.iter() {
                        match ctr.clone() {
                            CreateCtr(_id, config, ..) => {
                                writeln!(f, "        {}: CREATE {}", name, config.image)?;
                            }
                            StartCtr(id) => {
                                writeln!(f, "        {}: START {}", name, id)?;
                            }
                            StopCtr(id) => {
                                writeln!(f, "        {}: STOP {}", name, id)?;   
                            }
                            DeleteCtr(id) => {
                                writeln!(f, "        {}: DELETE {}", name, id)?;
                            }
                        }
                    }
                    writeln!(f, "    }}")?;
                }
                &DeletePod(ref id) => {
                    writeln!(f, "DELETE_POD {}", id)?;
                }
            }

        }
        writeln!(f, "}}")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {

}