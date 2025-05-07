use crate::Error;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug)]
pub struct SafeSender<T> {
    tx: mpsc::Sender<T>,
    timeout: Duration,
}

impl<T> Clone for SafeSender<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            timeout: self.timeout,
        }
    }
}

impl<T> SafeSender<T> {
    #[must_use]
    pub fn new(tx: mpsc::Sender<T>, timeout: Duration) -> Self {
        Self { tx, timeout }
    }

    /// # Errors
    ///
    /// Will return `Err` if timeout occured
    pub async fn safe_send(&self, data: T) -> Result<(), Error> {
        tokio::time::timeout(self.timeout, self.tx.send(data))
            .await
            .map_or(Err(Error::timeout()), |res| {
                res.map_or_else(|e| Err(Error::internal(e)), |()| Ok(()))
            })
    }
}
