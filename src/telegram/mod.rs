mod channel;
mod notice;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::mpsc;
use tokio::time::Instant;

pub use channel::{Channel, MAX_MESSAGE_CHARS, split};
pub use notice::{Notice, Topic, escape, visible};

const QUEUE_CAPACITY: usize = 256;
pub const MESSAGE_GAP: Duration = Duration::from_millis(1_100);
pub const MESSAGES_PER_MINUTE: usize = 20;

#[derive(Clone, Debug, Default, Serialize)]
pub struct TelegramView {
    pub configured: bool,
    pub delivered: u64,
    pub dropped: u64,
    pub last_error: Option<String>,
    pub last_delivered_at: Option<i64>,
}

#[derive(Clone)]
pub struct Notifier {
    queue: Option<mpsc::Sender<String>>,
    view: Arc<Mutex<TelegramView>>,
}

impl Notifier {
    pub fn disabled() -> Self {
        Self {
            queue: None,
            view: Arc::new(Mutex::new(TelegramView::default())),
        }
    }

    pub fn start(channel: Channel) -> Self {
        let (queue, receiver) = mpsc::channel(QUEUE_CAPACITY);
        let view = Arc::new(Mutex::new(TelegramView {
            configured: true,
            ..TelegramView::default()
        }));
        tokio::spawn(dispatch(channel, receiver, Arc::clone(&view)));
        Self {
            queue: Some(queue),
            view,
        }
    }

    pub fn send(&self, notice: &Notice) {
        self.send_html(notice.html());
    }

    pub fn send_html(&self, html: String) {
        let Some(queue) = &self.queue else {
            return;
        };
        if queue.try_send(html).is_err() {
            self.with_view(|view| view.dropped += 1);
        }
    }

    pub fn view(&self) -> TelegramView {
        self.view
            .lock()
            .map(|view| view.clone())
            .unwrap_or_default()
    }

    fn with_view(&self, change: impl FnOnce(&mut TelegramView)) {
        if let Ok(mut view) = self.view.lock() {
            change(&mut view);
        }
    }
}

pub fn coalesce(pending: &mut VecDeque<String>) -> Option<String> {
    let mut batch = pending.pop_front()?;
    while let Some(next) = pending.front() {
        if visible(&batch).chars().count() + visible(next).chars().count() + 2 > MAX_MESSAGE_CHARS {
            break;
        }
        batch.push_str("\n\n");
        batch.push_str(&pending.pop_front().expect("front exists"));
    }
    Some(batch)
}

async fn dispatch(
    channel: Channel,
    mut receiver: mpsc::Receiver<String>,
    view: Arc<Mutex<TelegramView>>,
) {
    let mut pending: VecDeque<String> = VecDeque::new();
    let mut sent_at: VecDeque<Instant> = VecDeque::new();
    loop {
        if pending.is_empty() {
            match receiver.recv().await {
                Some(text) => pending.push_back(text),
                None => return,
            }
        }
        while sent_at
            .front()
            .is_some_and(|at| at.elapsed() > Duration::from_secs(60))
        {
            sent_at.pop_front();
        }
        if sent_at.len() >= MESSAGES_PER_MINUTE {
            let oldest = *sent_at.front().expect("the window is full");
            tokio::time::sleep_until(oldest + Duration::from_secs(60)).await;
            continue;
        }
        if let Some(last) = sent_at.back() {
            tokio::time::sleep_until(*last + MESSAGE_GAP).await;
        }
        while let Ok(text) = receiver.try_recv() {
            pending.push_back(text);
        }
        let Some(batch) = coalesce(&mut pending) else {
            continue;
        };
        sent_at.push_back(Instant::now());
        let outcome = channel.send(&batch).await;
        if let Ok(mut view) = view.lock() {
            match outcome {
                Ok(()) => {
                    view.delivered += 1;
                    view.last_delivered_at = Some(crate::clock::now_unix());
                }
                Err(error) => {
                    tracing::warn!(%error, "a Telegram notice was not delivered");
                    view.last_error = Some(error.to_string());
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/telegram.rs"]
mod tests;
