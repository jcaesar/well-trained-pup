use std::{mem::take, sync::Arc};
use tokio::sync::{
    mpsc::{channel, Receiver, Sender},
    Mutex,
};
use well_trained_pup::Resp;

#[derive(Clone)]
pub struct Caster {
    receivers: Arc<Mutex<Vec<Sender<Resp>>>>,
    capacity: usize,
}

impl Caster {
    pub fn new(capacity: usize) -> Self {
        Self {
            receivers: Default::default(),
            capacity,
        }
    }
    pub async fn subscribe(&self) -> Receiver<Resp> {
        let (w, r) = channel(self.capacity);
        let mut receivers = self.receivers.lock().await;
        receivers.push(w);
        r
    }
    pub async fn send(&self, msg: Resp) {
        let mut receivers = self.receivers.lock().await;
        let mut retain = Vec::with_capacity(receivers.len());
        for r in take(&mut *receivers) {
            if r.send(msg.clone()).await.is_ok() {
                retain.push(r);
            }
        }
        *receivers = retain;
    }
}
