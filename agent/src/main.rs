mod common;
mod runtime;
mod state;
mod tasks;
mod worktree;
#[cfg(test)]
mod tests;

use tokio::sync::mpsc::{ Receiver, Sender };
use tokio::sync::watch::{Receiver as WatchRx, Sender as WatchTx};
use tokio::task::JoinSet;
use tokio::pin;

use common::*;

const CONTAINERD_SOCKET_PATH: &'static str = "/run/containerd/containerd.sock";
const EVENTS_BUFFER_MAX: usize = 100;
const STATE_REFRESH_INTERVAL: Duration = Duration::from_millis(20_000);
const EVENTS_RETRY_INTERVAL: Duration = Duration::from_millis(5_000);
const EVENTS_FLUSH_INTERVAL: Duration = Duration::from_millis(4_000);
const TARGET_REFRESH_INTERVAL: Duration = Duration::from_millis(15_000);

async fn poll_for_target(mut _target_tx: WatchTx<state::Target>) -> Result<(), Error> {
    loop {
        tokio::time::sleep(TARGET_REFRESH_INTERVAL).await;
    }
}

// There is low hanging fruit here for improvements in managing the amount of work done by the control loop and
// also managing the level of concurrency. For instance, because events happen to pods and contain the entire 
// state of the pod, they can be coalesced by pod, resulting in one message per pod for burst scenarios.
// Additionally, the number of messages sent to the control loop can be capped and the rest can be buffered.
// The latter is not a major concern for services that don't utilize much cpu anyway but later, for batch jobs,
// doing too much work in the agent can result in not enough cpu left over for jobs.
async fn read_events(mut rsc: RuntimeClient, ctr_events: Sender<Vec<cri::ContainerEventResponse>>) -> Result<(), Error> {
    loop {
        let mut events_resp = rsc.get_container_events().await.unwrap();
        let mut messages = vec![];
        loop {
            let timer = tokio::time::sleep(EVENTS_FLUSH_INTERVAL);
            pin!(timer);
            select! {
                message = events_resp.message() => {
                    match message {
                        Ok(Some(message)) => {
                            messages.push(message);
                        }
                        Ok(None) => {
                            ctr_events.send(messages).await.expect("Events receiver was suddenly dropped.");
                            messages = vec![];
                        }
                        _ => { break; }
                    }
                }
                _ = &mut timer => {
                    ctr_events.send(messages).await.expect("Events receiver was suddenly dropped.");
                    messages = vec![];
                }
            }
        }
        tokio::time::sleep(EVENTS_RETRY_INTERVAL).await;
    }
}

async fn control_loop(
    mut rsc: RuntimeClient,
    mut ctr_events: Receiver<Vec<cri::ContainerEventResponse>>,
    mut new_target: WatchRx<state::Target>

) -> Result<(), Error> {
    let mut target = state::Target::new();
    let mut state = state::State::new();
    let mut worktree = worktree::WorkTree::new();
    {
        let containers = rsc.list_containers().await?.containers;
        let pods = rsc.list_pods().await?.items;
        state.ingest(containers, pods);
    }
    let mut refresh_interval = tokio::time::interval(STATE_REFRESH_INTERVAL);
    loop {
        let mut rsc = rsc.clone();
        select! {
            events = ctr_events.recv() => {
                let events = events.expect("Events listener suddenly exited.");
                if events.len() == 0 { continue; }
                for event in events {
                    state.observe(event);
                }
            }
            _ = new_target.changed() => {
                target = new_target.borrow_and_update().clone();
            }
            _ = refresh_interval.tick() => {
                let containers = rsc.list_containers().await?.containers;
                let pods = rsc.list_pods().await?.items;
                state.ingest(containers, pods);
            }
        }
        let plan = state::diff(&target, &state);
        dbg!(&target);
        dbg!(&plan);
        worktree = worktree::execute(plan, worktree, &mut rsc);
    }
}

async fn agent() {
    let runtime = Cri::connect().await.expect("Could not connect to containerd.");
    let mut set = JoinSet::new();
    let (events_tx, events_rx) = tokio::sync::mpsc::channel(EVENTS_BUFFER_MAX);
    let (target_tx, target_rx) = tokio::sync::watch::channel(state::Target::new());

    set.spawn(poll_for_target(target_tx));
    set.spawn(read_events(runtime.clone(), events_tx));
    set.spawn(control_loop(runtime.clone(), events_rx, target_rx));

    let results = set.join_all().await;
    for result in results {
        if result.is_err() {
            log_err(result.unwrap_err());
        }
    }
}

#[tokio::main]
async fn main() {
    agent().await;
}