use std::future::Future;
use tokio::select;
use crate::common::*;

// TODO: Use in conjunction with ATTEMPTS metadata
// TODO: Exponential backoff
#[derive(PartialEq, Eq, Hash, Clone, Copy)]
pub enum RestartPolicy {
    Always,
    MaxAttempts(u64),
    Never
}

type CancelToken = tokio::sync::oneshot::Sender<()>;

// Supervisor for an ongoing CRI operation.
// Cancels the operation on drop.
pub struct Task {
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
    
    pub fn cancel(&mut self) {
        match self.cancel.take() {
            Some(tx) => { tokio::spawn(async { tx.send(()) }); }
            None => (),
        }
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        self.cancel();
    }
}