use crate::{
    common::*,
    tasks::*,
    state::*,
};

const CRI_RETRY_INTERVAL_MS: u64 = 200;

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
pub struct WorkTree {
    pods: HashMap<UID, PodTask>
}

impl WorkTree {
    pub fn new() -> WorkTree {
        WorkTree { pods: HashMap::new() }
    }
}

/// Convert a plan into a worktree of executing, cancellable tasks.
/// If we were already doing the task, move the task into the new worktree.
/// Otherwise, spawn the new task. The rest of them simply get dropped on the floor,
/// which triggers the cancel token. 
pub fn execute(plan: Plan, mut old_worktree: WorkTree, rsc: &mut RuntimeClient) -> WorkTree {
    use PodTask as PT;
    use PodStep as PS;
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
    fn spawn(self, rsc: RuntimeClient) -> PodTask {
        match self {
            Self::CreatePod(config) => {
                let ctor = move || {
                    let mut rsc = rsc.clone();
                    let config = config.clone();
                    async move { rsc.create_sandbox(config).await }
                };
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
                let ctor = move || {
                    let mut rsc = rsc.clone();
                    let pod_id = pod_id.clone();
                    async move { rsc.remove_pod(pod_id).await }
                };
                let task = Task::spawn(ctor, RestartPolicy::Always, CRI_RETRY_INTERVAL_MS);
                PodTask::DeletePod(task)
            }
        }
    }
}

impl crate::state::ContainerStep {
    fn spawn(self, rsc: RuntimeClient) -> ContainerTask {
        match self {
            Self::CreateCtr(pod_id, container_config, sandbox_config) => {
                let ctor = move || { 
                    let pod_id = pod_id.clone();
                    let container_config = container_config.clone();
                    let sandbox_config = sandbox_config.clone();
                    let mut rsc = rsc.clone();
                    async move {
                       rsc.pull_image(container_config.image.clone()).await?;
                       rsc.create_container(pod_id, container_config, sandbox_config).await
                    }
                };
                let task = Task::spawn(ctor, RestartPolicy::Always, CRI_RETRY_INTERVAL_MS);
                ContainerTask::CreateCtr(task)
            }
            Self::StartCtr(id) => {
                let ctor = move || {
                    let id = id.clone();
                    let mut rsc = rsc.clone();
                    async move { rsc.start_container(id).await }
                };
                let task = Task::spawn(ctor, RestartPolicy::Always, CRI_RETRY_INTERVAL_MS);
                ContainerTask::StartCtr(task)
            },
            Self::StopCtr(id) => {
                let ctor = move || {
                    let mut rsc = rsc.clone();
                    let id = id.clone();
                    async move { rsc.stop_container(id).await }
                };
                let task = Task::spawn(ctor, RestartPolicy::Always, CRI_RETRY_INTERVAL_MS);
                ContainerTask::StopCtr(task)
            },
            Self::DeleteCtr(id) => {
                let ctor = move || {
                    let id = id.clone();
                    let mut rsc = rsc.clone();
                    async move { rsc.remove_container(id).await }
                };
                let task = Task::spawn(ctor, RestartPolicy::Always, CRI_RETRY_INTERVAL_MS);
                ContainerTask::DeleteCtr(task)
            },
        }
    }
}