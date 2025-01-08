use k8s_cri::v1::ContainerEventResponse;
use k8s_cri::v1 as cri;

use std::sync::Arc;
use tokio::sync::Mutex;
use std::collections::HashMap;

fn to_state(i: i32) -> cri::ContainerState {
    cri::ContainerState::try_from(i).unwrap()
}

// todo: newtype these
type PodId = String;
type CtrId = String;
type Uid = String;
type Name = String;

#[derive(Debug)]
pub struct PodStatus {
    ctrs: Vec<CtrId> // This is really only so we can propagate deletion events to State.ctrs, which we won't receive on pod deletion
}

/// Something of a double indirection datastructure for keeping track of containerd's state
/// pods and ctrs map ids to known statuses. These are populated first by ListContainers etc. and then only updated by events.
/// uids and names map cluster names to actual containerd store objects. These are stitched together first by ListContainers
/// and then it's up to control_loop to manage names and garbage collect potential dangling ones.
/// Importantly, control_loop reads the pods and ctrs and executes operations to containerd but does NOT change the statuses. 
/// We periodically poll the state of containerd in case someone came along and made pods under our noses.
pub struct State {
    pub pods: HashMap<PodId, PodStatus>, // after initial list operations, ONLY ever populated by the event listener 
    pub ctrs: HashMap<CtrId, cri::ContainerState>, // after initial list operations, ONLY ever populated by the event listener 
    pub uids: HashMap<Uid, PodId>,       // after list op, only ever modified by control_loop
    pub names: HashMap<(Uid, Name), CtrId> // after list op, only ever modified by control_loop
}

pub type StateHandle = Arc<Mutex<State>>;

impl State {
    pub fn new() -> Arc<Mutex<Self>> {
        return Arc::new(Mutex::new(State {
            pods: HashMap::new(),
            ctrs: HashMap::new(),
            uids: HashMap::new(),
            names: HashMap::new(),
        }))
    }   

    /// Ingest the result of ListContainers and ListPodSandbox into the state,
    /// populating the name links as well.
    pub fn observe(&mut self, containers: Vec<cri::Container>, pods: Vec<cri::PodSandbox>) {
        let mut id_map: HashMap<PodId, Uid> = HashMap::new();
        for pod in pods {
            let metadata = pod.metadata.unwrap();
            self.pods.insert(pod.id.clone(), PodStatus {
                ctrs: vec![]
            });
            id_map.insert(pod.id.clone(), metadata.uid.clone());
            self.uids.insert(metadata.uid, pod.id);
        }
        
        for ctr in containers {
            let metadata = ctr.metadata.unwrap();
            let pod_uid = id_map.get(&ctr.pod_sandbox_id).unwrap().to_owned();
            let pod_id = &ctr.pod_sandbox_id;
            self.ctrs.insert(ctr.id.clone(), to_state(ctr.state));
            self.names.insert((pod_uid, metadata.name), ctr.id.clone());
            self.pods.get_mut(pod_id).unwrap().ctrs.push(ctr.id);
        }
    }

    // I had to experiment to figure out exactly what events are emitted during what combination of pod states and container events.
    // This is where we update the runtime state, so that on the control_loop's next iteration it can react to state changes.
    pub fn process_message(&mut self, message: ContainerEventResponse) {
        use cri::ContainerEventType as CE;
        use cri::ContainerState as CS;
        fn to_event(i: i32) -> CE {
            i.try_into().expect("Invalid container event type detected.")
        }
        let id = message.container_id.clone();
        // when the metadata's pod_id and the event's id match, this is a Pod start event
        if message.pod_sandbox_status.is_some() && message.pod_sandbox_status.as_ref().unwrap().id == id.clone() {
            match to_event(message.container_event_type) {
                CE::ContainerStartedEvent => {
                    self.pods.insert(id.clone(), PodStatus{ ctrs: vec![] });
                    #[cfg(debug_assertions)]
                    println!("EVENT: PodStarted: {}", id.clone());
                }
                _ => {}
            }
            return;
        }
        // When the metadata is none and the container event type is deletion, this is a Pod deletion event
        if message.pod_sandbox_status.is_none() {
            match to_event(message.container_event_type) {
                CE::ContainerDeletedEvent => {
                    match self.pods.remove(&id) {
                        None => {}
                        Some(PodStatus{ ctrs }) => {
                            // They're all gone, but we never received events for them.
                            for ctr in ctrs {
                                self.ctrs.remove(&ctr);
                            }
                        }
                    }
                    #[cfg(debug_assertions)]
                    println!("EVENT: PodDeleted: {}", id.clone());
                }
                _ => {}
            }
            return;
        }
        // This must be a container event.
        let pod_id = message.pod_sandbox_status.unwrap().id;
        let event_type = to_event(message.container_event_type);
        match event_type {
            CE::ContainerCreatedEvent => {
                self.ctrs.insert(id.clone(), CS::ContainerCreated);
                match self.pods.get_mut(&pod_id) {
                    Some(pod_status) => {
                        pod_status.ctrs.push(id.clone());
                    }
                    None => {}
                }
                #[cfg(debug_assertions)]
                println!("EVENT: ContainerCreated: {}", id.clone());
            }
            CE::ContainerStartedEvent => {
                self.ctrs.insert(id.clone(), CS::ContainerRunning);
                #[cfg(debug_assertions)]
                println!("EVENT: ContainerStarted: {}", id.clone());
            }
            CE::ContainerStoppedEvent => {
                self.ctrs.insert(id.clone(), CS::ContainerExited);
                #[cfg(debug_assertions)]
                println!("EVENT: ContainerStopped: {}", id.clone());
            }
            CE::ContainerDeletedEvent => {
                self.ctrs.remove(&id);
                match self.pods.get_mut(&pod_id) {
                    Some(pod_status) => {
                        match pod_status.ctrs.iter().position(|r| r == &id ) {
                            Some(i) => { pod_status.ctrs.swap_remove(i); }
                            None => {}
                        }
                    }
                    None => {} // pod mysteriously doesn't exist. oh well!
                }
                #[cfg(debug_assertions)]
                println!("EVENT: ContainerDeleted: {}", id.clone());
            }
        }
    }
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        f.write_str("uids: ")?;
        f.debug_map().entries(self.uids.iter()).finish()?;
        f.write_str("\n")?;

        f.write_str("pods: ")?;
        f.debug_map().entries(self.pods.iter()).finish()?;
        f.write_str("\n")?;

        f.write_str("names: ")?;
        f.debug_map().entries(self.names.iter()).finish()?;
        f.write_str("\n")?;

        f.write_str("ctrs: ")?;
        f.debug_map().entries(self.ctrs.iter()).finish()?;
        f.write_str("\n")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {

}