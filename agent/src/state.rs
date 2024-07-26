use k8s_cri::v1::ContainerEventResponse;
use k8s_cri::v1 as cri;

use std::sync::{Arc, Mutex};

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum ContainerState {
    Created,
    Running,
    Stopped,
    Unknown,
    Deleted
}

fn to_state(i: i32) -> ContainerState {
    use cri::ContainerState::*;
    match cri::ContainerState::try_from(i).unwrap() {
        ContainerCreated => ContainerState::Created,
        ContainerRunning => ContainerState::Running,
        ContainerExited => ContainerState::Stopped,
        ContainerUnknown => ContainerState::Unknown,
    }
}

pub struct State {
    containers: std::collections::HashMap<String, ContainerState>,
}

impl State {
    pub fn new() -> Arc<Mutex<Self>> {
        return Arc::new(Mutex::new(State {
            containers: std::collections::HashMap::new()
        }))
    }

    pub fn observe(&mut self, containers: Vec<cri::Container>) {
        use std::collections::hash_map::Entry;
        let mut old_cs = &mut self.containers;
        for c in containers {
            match old_cs.entry(c.id.clone()) {
                Entry::Occupied(e) => {
                    if *e.get() == ContainerState::Stopped {
                        continue;
                    }
                }
                Entry::Vacant(e) => {
                    let state = to_state(c.state);
                    e.insert(state);
                }
            }
        }
    }

    pub fn process_message(&mut self, message: ContainerEventResponse) {
        use cri::ContainerEventType as cri_event;
        let id = message.container_id;
        let event_type: cri::ContainerEventType = TryFrom::try_from(message.container_event_type).expect("Invalid container event type detected.");
        dbg!((id.clone()), event_type.clone());
        match event_type {
            cri_event::ContainerStartedEvent=> {
                self.containers.entry(id)
                    .and_modify(|e| {
                        *e = ContainerState::Running
                    })
                    .or_insert(
                        ContainerState::Running
                    );
            },
            cri_event::ContainerStoppedEvent => {
                self.containers.entry(id)
                    .and_modify(|e| {
                        *e = ContainerState::Stopped
                    })
                    .or_insert(
                        ContainerState::Stopped
                    );
            },
            cri_event::ContainerCreatedEvent => {
                self.containers.entry(id)
                    .and_modify(|e| {
                        *e = ContainerState::Created
                    })
                    .or_insert(
                        ContainerState::Created
                    );
            },
            cri_event::ContainerDeletedEvent => {
                self.containers.entry(id)
                    .and_modify(|e| {
                        *e = ContainerState::Deleted
                    })
                    .or_insert(
                        ContainerState::Deleted
                    );
            }
        }
    }
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        f.debug_map().entries(self.containers.iter()).finish()?;
        Ok(())
    }
}