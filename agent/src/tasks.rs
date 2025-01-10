use std::future::Future;

use tokio::{runtime::Runtime, select};

use crate::{common::*, state::Plan};

const CRI_RETRY_INTERVAL_MS: u64 = 200;

// TODO: Use in conjunction with ATTEMPTS metadata
// TODO: Exponential backoff
#[derive(PartialEq, Eq, Hash, Clone, Copy)]
enum RestartPolicy {
    Always,
    MaxAttempts(u64),
    Never
}

type CancelToken = tokio::sync::oneshot::Sender<()>;

// Supervisor for an ongoing CRI operation.
// Cancels the operation on drop.
struct Task {
    handle: tokio::task::JoinHandle<()>,
    cancel: Option<CancelToken>
}

impl Task {
    // Need a ctor to produce a future to enable retries.
    pub fn spawn<Ctor, F, E, T>(mut ctor: Ctor, restart_policy: RestartPolicy, retry_interval_ms: u64) -> Task 
        where F: Future<Output = Result<T, E>> + Send + 'static,
              Ctor: FnMut() -> F + Send + 'static,
              E: Send + std::error::Error + 'static,
              T: Send + 'static,
    {
        use RestartPolicy::*;
        let mut attempts = 0;
        let attempt_max = match restart_policy {
            Always => std::u64::MAX,
            MaxAttempts(i) => i,
            Never => 1,
        };

        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();
        let supervisor = async move {
            loop {
                let mut request_handle = tokio::spawn(ctor());
                select! {
                    _ = &mut cancel_rx => {
                        request_handle.abort();
                        break;
                    }
                    result = &mut request_handle => {
                        match result {
                            Ok(Ok(_)) => { break; }  // successfully completed
                            Ok(Err(e)) => {
                                log_err(e);           // operation failed...
                            }  
                            Err(e) => {               // thread panicked
                                log_err(e);       // todo: wrap errors
                            }
                        }
                    }
                }
                attempts += 1;
                if attempts >= attempt_max { break; }
                tokio::time::sleep(Duration::from_millis(retry_interval_ms)).await;
            }
        };
        let supervisor_handle = tokio::spawn(supervisor);
        
        Task { handle: supervisor_handle, cancel: Some(cancel_tx) }
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        match self.cancel.take() {
            Some(tx) => { tokio::spawn(async { tx.send(()) }); }
            None => (),
        }
    }
}

enum PodTask {
    CreatePod(Task),
    ChangePod(HashMap<Name, ContainerTask>),
    DeletePod(Task),
}

enum ContainerTask {
    CreateCtr(Task),
    StartCtr(Task),
    StopCtr(Task),
    DeleteCtr(Task),
}

/// A tree of Tasks executing the aforementioned plan
struct WorkTree {
    pub pods: HashMap<UID, PodTask>
}

/// Convert a plan into a worktree of executing, cancellable tasks.
/// If we were already doing the task, move the task into the new worktree.
/// Otherwise, spawn the new task. The rest of them simply get dropped on the floor,
/// which triggers the cancel token. 
pub fn execute(plan: Plan, mut old_worktree: WorkTree, rsc: &mut RuntimeService) -> WorkTree {
    use PodTask as PT;
    use crate::PodStep as PS;
    let mut new_worktree = WorkTree { pods: HashMap::new() };
    for (uid, pod_step) in plan.pods {
        match (pod_step, old_worktree.pods.remove(&uid)) {
            (PS::CreatePod(_), Some(PT::CreatePod(task))) => {
                new_worktree.pods.insert(uid.clone(), PT::CreatePod(task));
            }
            (PS::DeletePod(_), Some(PT::DeletePod(task))) => {
                new_worktree.pods.insert(uid.clone(), PT::DeletePod(task));
            }
            (PS::ChangePod(steps), Some(PT::ChangePod(mut old_tasks))) => {
                let tasks = steps.into_iter()
                    .map(|(name, step)| {
                        match old_tasks.remove(&name) {
                            None => (name, step.spawn(rsc.clone())),
                            Some(task) => (name, task),
                        }
                    })
                    .collect();
                new_worktree.pods.insert(uid.clone(), PT::ChangePod(tasks));
            }
            (pod_step, _) => {
                new_worktree.pods.insert(uid.clone(), pod_step.spawn(rsc.clone()));
            }
        }
    }
    new_worktree
}

impl crate::state::PodStep {
    pub fn spawn(self, rsc: RuntimeService) -> PodTask {
        match self {
            Self::CreatePod(config) => {
                let ctor = move || { crate::operations::create_sandbox(rsc.clone(), config.clone()) };
                let task = Task::spawn(ctor, RestartPolicy::Always, CRI_RETRY_INTERVAL_MS);
                PodTask::CreatePod(task)
            }
            Self::ChangePod(names) => {
                let mut tasks = HashMap::new();
                for (name, step) in names {
                    let rsc = rsc.clone();
                    tasks.insert(name, step.spawn(rsc));
                }
                PodTask::ChangePod(tasks)
            }
            Self::DeletePod(pod_id) => {
                let ctor = move || { crate::operations::remove_pod(rsc.clone(), pod_id.clone()) };
                let task = Task::spawn(ctor, RestartPolicy::Always, CRI_RETRY_INTERVAL_MS);
                PodTask::DeletePod(task)
            }
        }
    }
}

impl crate::state::ContainerStep {
    pub fn spawn(self, rsc: RuntimeService) -> ContainerTask {
        match self {
            Self::CreateCtr(pod_id, container_config, sandbox_config) => {
                let ctor = move || { crate::operations::create_container(
                    rsc.clone(),
                    pod_id.clone(),
                    container_config.clone(),
                    sandbox_config.clone()
                ) };
                let task = Task::spawn(ctor, RestartPolicy::Always, CRI_RETRY_INTERVAL_MS);
                ContainerTask::CreateCtr(task)
            }
            Self::StartCtr(id) => {
                let ctor = move || { crate::operations::start_container(rsc.clone(), id.clone()) };
                let task = Task::spawn(ctor, RestartPolicy::Always, CRI_RETRY_INTERVAL_MS);
                ContainerTask::StartCtr(task)
            },
            Self::StopCtr(id) => {
                let ctor = move || { crate::operations::stop_container(rsc.clone(), id.clone()) };
                let task = Task::spawn(ctor, RestartPolicy::Always, CRI_RETRY_INTERVAL_MS);
                ContainerTask::StopCtr(task)
            },
            Self::DeleteCtr(id) => {
                let ctor = move || { crate::operations::remove_container(rsc.clone(), id.clone()) };
                let task = Task::spawn(ctor, RestartPolicy::Always, CRI_RETRY_INTERVAL_MS);
                ContainerTask::StopCtr(task)
            },
        }
    }
}