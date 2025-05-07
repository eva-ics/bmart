// TODO logs
use crate::Error;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Notify};
use tokio::task;
use tokio::time::{sleep_until, Instant};

const ERR_DUPLICATE_WORKER_ID: &str = "Duplicate worker ID";
const ERR_WORKER_NOT_FOUND: &str = "Worker not found";

#[derive(Debug)]
pub struct Scheduler {
    interval: Duration,
    trigger: Arc<Notify>,
}

impl Scheduler {
    pub fn new(trigger: Arc<Notify>, interval: Duration) -> Self {
        Self { interval, trigger }
    }
    pub async fn run(&mut self) {
        let mut t = Instant::now();
        loop {
            t += self.interval;
            sleep_until(t).await;
            self.trigger.notify_waiters();
        }
    }
    pub async fn run_instant(&mut self) {
        let mut t = Instant::now();
        loop {
            self.trigger.notify_waiters();
            t += self.interval;
            sleep_until(t).await;
        }
    }
}

pub struct WorkerFactory {
    schedulers: BTreeMap<String, task::JoinHandle<()>>,
}

impl Default for WorkerFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkerFactory {
    #[must_use]
    pub fn new() -> Self {
        Self {
            schedulers: BTreeMap::new(),
        }
    }

    /// # Errors
    ///
    /// Will return `Err` if the worker already exists
    pub fn create_scheduler(
        &mut self,
        worker_id: &str,
        trigger: Arc<Notify>,
        interval: Duration,
        instant: bool,
    ) -> Result<(), Error> {
        self._create_scheduler(worker_id, trigger, interval, false, instant)
    }

    /// # Errors
    ///
    /// Will return `Err` if failed to recreate the worker
    pub fn recreate_scheduler(
        &mut self,
        worker_id: &str,
        trigger: Arc<Notify>,
        interval: Duration,
        instant: bool,
    ) -> Result<(), Error> {
        self._create_scheduler(worker_id, trigger, interval, true, instant)
    }

    fn _create_scheduler(
        &mut self,
        worker_id: &str,
        trigger: Arc<Notify>,
        interval: Duration,
        recreate: bool,
        instant: bool,
    ) -> Result<(), Error> {
        if self.schedulers.contains_key(worker_id) {
            if recreate {
                let _r = self.destroy_scheduler(worker_id);
            } else {
                return Err(Error::duplicate(ERR_DUPLICATE_WORKER_ID));
            }
        }
        let mut scheduler = Scheduler::new(trigger, interval);
        let fut = if instant {
            tokio::spawn(async move {
                scheduler.run_instant().await;
            })
        } else {
            tokio::spawn(async move {
                scheduler.run().await;
            })
        };
        self.schedulers.insert(worker_id.to_owned(), fut);
        Ok(())
    }

    /// # Errors
    ///
    /// Will return `Err` if the worker does not exist
    pub fn destroy_scheduler(&mut self, worker_id: &str) -> Result<(), Error> {
        self.schedulers.remove(worker_id).map_or(
            Err(Error::not_found(ERR_WORKER_NOT_FOUND)),
            |fut| {
                fut.abort();
                Ok(())
            },
        )
    }
}

pub struct TaskWorker<F, Fut, T>
where
    F: FnMut(T) -> Fut,
    Fut: std::future::Future<Output = ()>,
    T: Sync + fmt::Debug,
{
    func: F,
    rx: mpsc::Receiver<T>,
}

impl<F, Fut, T> TaskWorker<F, Fut, T>
where
    F: FnMut(T) -> Fut,
    Fut: std::future::Future<Output = ()>,
    T: Sync + fmt::Debug,
{
    pub fn new(func: F, buf: usize) -> (Self, mpsc::Sender<T>) {
        let (tx, rx) = mpsc::channel(buf);
        (Self { func, rx }, tx)
    }

    pub async fn run(&mut self) {
        while let Some(v) = self.rx.recv().await {
            (self.func)(v).await;
        }
    }
}
