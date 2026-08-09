use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use rand::Rng;
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::models::{Listing, Profile};

pub type PollFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = Vec<Listing>> + Send + 'static>>;
pub type PollCallback = Arc<dyn Fn(Profile) -> PollFuture + Send + Sync>;

/// How long stop() waits for the in-flight poll to finish on its own
/// before aborting the worker outright.
const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);

/// Longest the idle worker sleeps before re-checking state, even when the
/// next scheduled poll is further out — keeps it responsive to stop().
const MAX_IDLE_SLEEP: Duration = Duration::from_secs(30);

struct ProfileJob {
    profile: Profile,
    callback: PollCallback,
    /// When this profile is next eligible to poll. `None` after a manual
    /// trigger, meaning "run at the next opportunity, ahead of scheduled
    /// work".
    next_due: Option<DateTime<Utc>>,
    forced: bool,
}

/// Sequential poll scheduler: a single worker walks the registered
/// profiles in registration (config) order and polls due ones **one at a
/// time**, fully, before moving to the next. There is never more than one
/// poll in flight, which is both easy to reason about and the gentlest
/// possible footprint against the marketplaces. Manual triggers jump ahead
/// of scheduled polls but still wait for the current one to finish.
pub struct PollScheduler {
    // Registration order is the poll order, so this is an ordered Vec, not
    // a map — lookups are linear over a small (~10s) profile set.
    jobs: Arc<Mutex<Vec<ProfileJob>>>,
    running: Arc<std::sync::atomic::AtomicBool>,
    wake: Arc<Notify>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl Default for PollScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl PollScheduler {
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(Mutex::new(Vec::new())),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            wake: Arc::new(Notify::new()),
            worker: Mutex::new(None),
        }
    }

    pub fn running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Apply ±10% jitter to a profile's configured interval so cadence is
    /// not clockwork-regular.
    fn jittered_interval(profile: &Profile) -> chrono::Duration {
        let base = profile.poll_interval_sec as f64;
        let jitter = rand::thread_rng().gen_range(-0.1..0.1);
        chrono::Duration::seconds(((base * (1.0 + jitter)).max(1.0)) as i64)
    }

    /// Spawn the single sequential worker.
    pub async fn start(&self) {
        self.running
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let jobs = self.jobs.clone();
        let running = self.running.clone();
        let wake = self.wake.clone();
        let handle = tokio::spawn(async move {
            Self::run_loop(jobs, running, wake).await;
        });
        *self.worker.lock().await = Some(handle);
    }

    async fn run_loop(
        jobs: Arc<Mutex<Vec<ProfileJob>>>,
        running: Arc<std::sync::atomic::AtomicBool>,
        wake: Arc<Notify>,
    ) {
        while running.load(std::sync::atomic::Ordering::Relaxed) {
            // Select the next job to run, and reschedule it, while holding
            // the lock — so it can't be picked twice or race a reload.
            let (selected, sleep_until) = {
                let mut jobs = jobs.lock().await;
                let now = Utc::now();

                // Forced (manually triggered) jobs first, in order.
                let idx = jobs
                    .iter()
                    .position(|j| j.forced)
                    .or_else(|| {
                        // Otherwise the first job whose scheduled time has
                        // arrived, in registration order.
                        jobs.iter().position(|j| match j.next_due {
                            Some(due) => due <= now,
                            None => true,
                        })
                    });

                match idx {
                    Some(i) => {
                        let job = &mut jobs[i];
                        job.forced = false;
                        job.next_due = Some(now + Self::jittered_interval(&job.profile));
                        (Some((job.callback.clone(), job.profile.clone())), None)
                    }
                    None => {
                        // Nothing due — sleep until the soonest next_due
                        // (capped), or until woken by a trigger/registration.
                        let soonest = jobs
                            .iter()
                            .filter_map(|j| j.next_due)
                            .min()
                            .map(|due| (due - now).to_std().unwrap_or(Duration::ZERO))
                            .unwrap_or(MAX_IDLE_SLEEP);
                        (None, Some(soonest.min(MAX_IDLE_SLEEP).max(Duration::from_millis(100))))
                    }
                }
            };

            match selected {
                // Run the poll to completion before looping — this is what
                // makes the whole system sequential.
                Some((cb, profile)) => {
                    let id = profile.id.clone();
                    let fut = cb(profile);
                    fut.await;
                    // Yield so a stop() issued during the poll is observed
                    // promptly on the next guard check.
                    if !running.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                    let _ = id;
                }
                None => {
                    let dur = sleep_until.unwrap_or(MAX_IDLE_SLEEP);
                    tokio::select! {
                        _ = tokio::time::sleep(dur) => {}
                        _ = wake.notified() => {}
                    }
                }
            }
        }
    }

    /// Stop the worker: signal it to exit, wake it out of any idle sleep,
    /// then wait (bounded) for the current poll — fetch, evaluate, DB
    /// writes, Chrome tab cleanup — to finish before returning. Anything
    /// still running past the timeout is aborted rather than silently
    /// killed when the runtime drops.
    pub async fn stop(&self) {
        self.running
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.wake.notify_waiters();
        let handle = self.worker.lock().await.take();
        if let Some(handle) = handle {
            match tokio::time::timeout(SHUTDOWN_DRAIN_TIMEOUT, handle).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) if e.is_panic() => warn!(error = %e, "poll worker panicked"),
                Ok(Err(_)) => {}
                Err(_) => {
                    warn!("in-flight poll did not finish within the shutdown timeout, aborting");
                    // The timeout consumed the handle; nothing further to
                    // abort explicitly — the runtime drop will reap it, and
                    // we've already logged the abandonment.
                }
            }
        }
    }

    /// True if a job for this profile id is registered with an identical
    /// definition. Reload uses this to skip re-registering unchanged
    /// profiles, which would otherwise reset their schedule.
    pub async fn job_matches(&self, profile: &Profile) -> bool {
        let jobs = self.jobs.lock().await;
        jobs.iter().find(|j| j.profile.id == profile.id).is_some_and(|j| {
            serde_json::to_value(&j.profile).ok() == serde_json::to_value(profile).ok()
        })
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
        let interval = Self::jittered_interval(&profile);
        let now = Utc::now();
        // Restart-aware: a profile polled recently is due at last+interval,
        // so a daemon restart (or reload) doesn't re-poll it immediately.
        let next_due = match last_polled {
            Some(last) if last + interval > now => {
                let due = last + interval;
                info!(
                    profile = %profile.id,
                    next_in_secs = (due - now).num_seconds().max(0),
                    "profile recently polled, scheduling next run"
                );
                Some(due)
            }
            _ => Some(now),
        };

        let mut jobs = self.jobs.lock().await;
        if let Some(existing) = jobs.iter_mut().find(|j| j.profile.id == profile.id) {
            existing.profile = profile;
            existing.callback = callback;
            existing.next_due = next_due;
            existing.forced = false;
        } else {
            jobs.push(ProfileJob {
                profile,
                callback,
                next_due,
                forced: false,
            });
        }
        self.wake.notify_waiters();
    }

    pub async fn remove_profile(&self, profile_id: &str) {
        let mut jobs = self.jobs.lock().await;
        jobs.retain(|j| j.profile.id != profile_id);
    }

    pub async fn has_job(&self, profile_id: &str) -> bool {
        let jobs = self.jobs.lock().await;
        jobs.iter().any(|j| j.profile.id == profile_id)
    }

    /// Queue a profile to poll ahead of scheduled work. It still runs
    /// after any poll currently in flight — the system stays sequential.
    pub async fn trigger_now(&self, profile_id: &str) {
        if !self.running.load(std::sync::atomic::Ordering::Relaxed) {
            warn!(profile = %profile_id, "scheduler stopped, ignoring trigger_now");
            return;
        }
        let mut jobs = self.jobs.lock().await;
        match jobs.iter_mut().find(|j| j.profile.id == profile_id) {
            Some(job) => job.forced = true,
            None => {
                warn!(profile = %profile_id, "no job registered for profile");
                return;
            }
        }
        drop(jobs);
        self.wake.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AlertPriority, KeywordGroup};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn make_profile(id: &str, poll_interval_sec: u64) -> Profile {
        Profile {
            id: id.to_string(),
            name: id.to_string(),
            keywords: vec![KeywordGroup::Single("x".into())],
            negative_keywords: vec![],
            sources: vec![],
            price_min: None,
            price_max: None,
            poll_interval_sec,
            alert_priority: AlertPriority::Normal,
            enabled: true,
            tags: vec![],
            escalation_keywords: vec![],
            location_radius_mi: None,
        }
    }

    fn counting_callback(counter: Arc<AtomicUsize>) -> PollCallback {
        Arc::new(move |_p: Profile| {
            let counter = counter.clone();
            Box::pin(async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Vec::new()
            })
        })
    }

    /// Records the id of every profile polled, in order, so ordering can be
    /// asserted.
    fn recording_callback(log: Arc<Mutex<Vec<String>>>) -> PollCallback {
        Arc::new(move |p: Profile| {
            let log = log.clone();
            Box::pin(async move {
                log.lock().await.push(p.id.clone());
                Vec::new()
            })
        })
    }

    async fn settle() {
        for _ in 0..100 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn polls_all_due_profiles_in_registration_order() {
        let scheduler = PollScheduler::new();
        scheduler.start().await;
        let order = Arc::new(Mutex::new(Vec::new()));
        for id in ["p1", "p2", "p3"] {
            scheduler
                .add_profile(make_profile(id, 3600), recording_callback(order.clone()), None)
                .await;
        }
        // Give the sequential worker time to drain the three due profiles.
        for _ in 0..200 {
            if order.lock().await.len() >= 3 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        scheduler.stop().await;
        assert_eq!(*order.lock().await, vec!["p1", "p2", "p3"]);
    }

    #[tokio::test]
    async fn never_polls_two_profiles_concurrently() {
        let scheduler = PollScheduler::new();
        scheduler.start().await;
        let current = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        let cb: PollCallback = {
            let current = current.clone();
            let max_seen = max_seen.clone();
            Arc::new(move |_p: Profile| {
                let current = current.clone();
                let max_seen = max_seen.clone();
                Box::pin(async move {
                    let n = current.fetch_add(1, Ordering::SeqCst) + 1;
                    max_seen.fetch_max(n, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(15)).await;
                    current.fetch_sub(1, Ordering::SeqCst);
                    Vec::new()
                })
            })
        };
        for i in 0..5 {
            scheduler
                .add_profile(make_profile(&format!("p{i}"), 3600), cb.clone(), None)
                .await;
        }
        for _ in 0..200 {
            if max_seen.load(Ordering::SeqCst) > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        tokio::time::sleep(Duration::from_millis(120)).await;
        scheduler.stop().await;
        assert_eq!(max_seen.load(Ordering::SeqCst), 1, "polls must never overlap");
    }

    #[tokio::test]
    async fn recently_polled_profile_is_not_due_immediately() {
        let scheduler = PollScheduler::new();
        scheduler.start().await;
        let recent = Arc::new(AtomicUsize::new(0));
        let fresh = Arc::new(AtomicUsize::new(0));
        scheduler
            .add_profile(make_profile("recent", 3600), counting_callback(recent.clone()), Some(Utc::now()))
            .await;
        scheduler
            .add_profile(make_profile("fresh", 3600), counting_callback(fresh.clone()), None)
            .await;
        for _ in 0..200 {
            if fresh.load(Ordering::SeqCst) >= 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
        scheduler.stop().await;
        assert_eq!(fresh.load(Ordering::SeqCst), 1, "never-polled profile runs promptly");
        assert_eq!(recent.load(Ordering::SeqCst), 0, "recently-polled profile waits its interval");
    }

    #[tokio::test]
    async fn trigger_now_after_stop_does_not_fire() {
        let scheduler = PollScheduler::new();
        scheduler.start().await;
        let counter = Arc::new(AtomicUsize::new(0));
        scheduler
            .add_profile(make_profile("p", 3600), counting_callback(counter.clone()), Some(Utc::now()))
            .await;
        settle().await;
        scheduler.stop().await;
        let before = counter.load(Ordering::SeqCst);
        scheduler.trigger_now("p").await;
        settle().await;
        assert_eq!(counter.load(Ordering::SeqCst), before, "trigger_now after stop must not poll");
    }

    #[tokio::test]
    async fn trigger_now_polls_a_not_yet_due_profile() {
        let scheduler = PollScheduler::new();
        scheduler.start().await;
        let counter = Arc::new(AtomicUsize::new(0));
        // Recently polled — would not be due for ~an hour.
        scheduler
            .add_profile(make_profile("p", 3600), counting_callback(counter.clone()), Some(Utc::now()))
            .await;
        settle().await;
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        scheduler.trigger_now("p").await;
        for _ in 0..200 {
            if counter.load(Ordering::SeqCst) >= 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        scheduler.stop().await;
        assert_eq!(counter.load(Ordering::SeqCst), 1, "manual trigger runs a not-yet-due profile");
    }
}
