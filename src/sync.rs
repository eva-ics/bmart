use crate::Error;
use std::collections::{btree_map, BTreeMap};
use std::sync::atomic;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tokio::task;
use uuid::Uuid;

const ERR_LOCK_NOT_DEFINED: &str = "Lock not defined";
const ERR_INVALID_LOCK_TOKEN: &str = "Invalid lock token";

#[derive(Debug, Clone)]
pub struct Lock {
    unlock_trigger: mpsc::Sender<()>,
}

impl Lock {
    /// Returns true if released, false if not locked
    pub async fn release(&self) -> bool {
        self.unlock_trigger.send(()).await.is_ok()
    }
}

#[derive(Debug, Default)]
pub struct SharedLock {
    lock: Arc<Mutex<()>>,
    flag: Arc<atomic::AtomicBool>,
}

impl SharedLock {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    pub async fn acquire(&self, expires: Duration) -> Lock {
        let lock = self.lock.clone();
        let (lock_trigger, lock_listener) = triggered::trigger();
        let (unlock_trigger, mut unlock_listener) = mpsc::channel(1);
        let flag = self.flag.clone();
        task::spawn(async move {
            // guard moved here
            let _g = lock.lock().await;
            // triggered as soon as the lock is acquired
            flag.store(true, atomic::Ordering::SeqCst);
            lock_trigger.trigger();
            // exited as soon as unlocked or expired or unlock_trigger dropped
            let _ = tokio::time::timeout(expires, unlock_listener.recv()).await;
            flag.store(false, atomic::Ordering::SeqCst);
        });
        // want lock to be acquired
        lock_listener.await;
        Lock { unlock_trigger }
    }
    pub fn clone_flag(&self) -> Arc<atomic::AtomicBool> {
        self.flag.clone()
    }
}

#[derive(Debug, Default)]
pub struct SharedLockFactory {
    shared_locks: BTreeMap<String, (Mutex<SharedLock>, Arc<atomic::AtomicBool>)>,
    locks: Mutex<BTreeMap<String, (Uuid, Lock)>>,
}

impl SharedLockFactory {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    /// # Errors
    ///
    /// Will return `Err` if the lock already exists
    pub fn create(&mut self, lock_id: &str) -> Result<(), Error> {
        if let btree_map::Entry::Vacant(x) = self.shared_locks.entry(lock_id.to_owned()) {
            let slock = SharedLock::new();
            let flag = slock.clone_flag();
            x.insert((Mutex::new(slock), flag));
            Ok(())
        } else {
            Err(Error::duplicate(format!(
                "Shared lock {} already exists",
                lock_id
            )))
        }
    }
    /// # Errors
    ///
    /// Will return `Err` if the lock is not defined
    pub async fn acquire(&self, lock_id: &str, expires: Duration) -> Result<Uuid, Error> {
        if let Some((v, _)) = self.shared_locks.get(lock_id) {
            // wait for the lock and block other futures accessing it
            let lock = v.lock().await.acquire(expires).await;
            let token = Uuid::new_v4();
            self.locks
                .lock()
                .await
                .insert(lock_id.to_owned(), (token, lock));
            Ok(token)
        } else {
            Err(Error::not_found(ERR_LOCK_NOT_DEFINED))
        }
    }
    /// # Errors
    ///
    /// Will return `Err` if the token is invalid, None forcibly releases the lock
    pub async fn release(&self, lock_id: &str, token: Option<&Uuid>) -> Result<bool, Error> {
        if let Some((tok, lock)) = self.locks.lock().await.get(lock_id) {
            if let Some(t) = token {
                if tok != t {
                    return Err(Error::not_found(ERR_INVALID_LOCK_TOKEN));
                }
            }
            Ok(lock.release().await)
        } else {
            Err(Error::not_found(ERR_LOCK_NOT_DEFINED))
        }
    }
    /// # Errors
    ///
    /// Will return `Err` if the lock is not defined
    pub fn status(&self, lock_id: &str) -> Result<bool, Error> {
        if let Some((_, flag)) = self.shared_locks.get(lock_id) {
            Ok(flag.load(atomic::Ordering::SeqCst))
        } else {
            Err(Error::not_found(ERR_LOCK_NOT_DEFINED))
        }
    }
    pub fn list(&self) -> Vec<(&str, bool)> {
        let mut result = Vec::new();
        for (id, (_, flag)) in &self.shared_locks {
            result.push((id.as_str(), flag.load(atomic::Ordering::SeqCst)));
        }
        result
    }
}
