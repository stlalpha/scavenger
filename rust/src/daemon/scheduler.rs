use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use rand::Rng;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::models::{Listing, Profile};

pub type PollFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = Vec<Listing>> + Send + 'static>>;
pub type PollCallback = Arc<dyn Fn(Profile) -> PollFuture + Send + Sync>;

struct ProfileJob {
    profile: Profile,
    callback: PollCallback,
    handle: JoinHandle<()>,
}

pub struct PollScheduler {
    jobs: Arc<Mutex<HashMap<String, ProfileJob>>>,
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl PollScheduler {
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(Mutex::new(HashMap::new())),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub fn running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub async fn start(&self) {
        self.running
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub async fn stop(&self) {
        self.running
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let mut jobs = self.jobs.lock().await;
        for (_, job) in jobs.drain() {
            job.handle.abort();
        }
    }

    pub async fn add_profile(
        &self,
        profile: Profile,
        callback: PollCallback,
        last_polled: Option<DateTime<Utc>>,
    ) {
        if !profile.enabled {
            return;
        }

        let profile_id = profile.id.clone();

        // Remove existing job if any
        {
            let mut jobs = self.jobs.lock().await;
            if let Some(old) = jobs.remove(&profile_id) {
                old.handle.abort();
            }
        }

        // Apply +-10% jitter to the interval
        let base_interval = profile.poll_interval_sec as f64;
        let jitter = rand::thread_rng().gen_range(-0.1..0.1);
        let interval_secs = (base_interval * (1.0 + jitter)).max(1.0) as u64;

        // Calculate initial delay
        let now = Utc::now();
        let initial_delay = if let Some(last) = last_polled {
            let next_run = last + chrono::Duration::seconds(interval_secs as i64);
            if next_run > now {
                let delay = (next_run - now).num_seconds().max(0) as u64;
                info!(
                    profile = %profile_id,
                    last_polled_secs_ago = (now - last).num_seconds(),
                    next_in_secs = delay,
                    "profile recently polled, delaying next run"
                );
                delay
            } else {
                0
            }
        } else {
            0
        };

        let cb = callback.clone();
        let p = profile.clone();
        let running = self.running.clone();

        let handle = tokio::spawn(async move {
            // Initial delay before first poll
            if initial_delay > 0 {
                tokio::time::sleep(tokio::time::Duration::from_secs(initial_delay)).await;
            }

            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(interval_secs));
            // First tick fires immediately (or after initial delay above)
            loop {
                interval.tick().await;
                if !running.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let fut = cb(p.clone());
                if let Err(e) = tokio::spawn(fut).await {
                    warn!(profile = %p.id, error = %e, "poll task panicked");
                }
            }
        });

        let mut jobs = self.jobs.lock().await;
        jobs.insert(
            profile_id,
            ProfileJob {
                profile,
                callback,
                handle,
            },
        );
    }

    pub async fn remove_profile(&self, profile_id: &str) {
        let mut jobs = self.jobs.lock().await;
        if let Some(job) = jobs.remove(profile_id) {
            job.handle.abort();
        }
    }

    pub async fn has_job(&self, profile_id: &str) -> bool {
        let jobs = self.jobs.lock().await;
        jobs.contains_key(profile_id)
    }

    pub async fn trigger_now(&self, profile_id: &str) {
        let jobs = self.jobs.lock().await;
        if let Some(job) = jobs.get(profile_id) {
            let cb = job.callback.clone();
            let p = job.profile.clone();
            tokio::spawn(async move {
                let fut = cb(p);
                fut.await;
            });
        } else {
            warn!(profile = %profile_id, "no callback registered for profile");
        }
    }
}
