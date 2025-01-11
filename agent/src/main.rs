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

use common::*;

const CONTAINERD_SOCKET_PATH: &'static str = "/run/containerd/containerd.sock";
const EVENTS_BUFFER_MAX: usize = 10_000;
const CONTROL_LOOP_INTERVAL: Duration = Duration::from_millis(1_000);
const STATE_REFRESH_INTERVAL: Duration = Duration::from_millis(60_000);
const EVENTS_REFRESH_INTERVAL: Duration = Duration::from_millis(3_000);
const TARGET_REFRESH_INTERVAL: Duration = Duration::from_millis(15_000);

async fn poll_for_target(mut _target_tx: WatchTx<state::Target>) -> Result<(), Error> {
    loop {
        tokio::time::sleep(TARGET_REFRESH_INTERVAL).await;
    }
}

async fn read_all_messages(runtime_service: &mut RuntimeClient) -> Vec<cri::ContainerEventResponse> {
    // TODO: Refactor this to not open a connection every three seconds.. grpc streams can be open indefinitely
    let mut events_resp = runtime_service.get_container_events()
        .await
        .expect("Could not get events stream.");
    let mut messages = vec![];
    while let Ok(Some(message)) = events_resp.message().await {
        messages.push(message);
    }
    messages
}

async fn read_events(mut runtime_service: RuntimeClient, ctr_events: Sender<Vec<cri::ContainerEventResponse>>) -> Result<(), Error> {
    loop {
        let messages = read_all_messages(&mut runtime_service).await;
        ctr_events.send(messages).await.expect("Channel suddenly dropped.");
        tokio::time::sleep(EVENTS_REFRESH_INTERVAL).await;
    }
}

async fn drain_messages(rsc: &mut RuntimeClient) {
    let _ = read_all_messages(rsc).await;
}

async fn control_loop(
    mut rsc: RuntimeClient,
    mut ctr_events: Receiver<Vec<cri::ContainerEventResponse>>,
    mut new_target: WatchRx<state::Target>

) -> Result<(), Error> {
    drain_messages(&mut rsc).await;
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
        worktree = worktree::execute(plan, worktree, &mut rsc);
        tokio::time::sleep(CONTROL_LOOP_INTERVAL).await;
    }
}

async fn agent() {
    let runtime = RuntimeClient::connect().await.expect("Could not connect to containerd.");
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