use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use chrono::Utc;

use scavenger::daemon::scheduler::{PollCallback, PollScheduler};
use scavenger::models::Profile;

fn test_profile(id: &str, interval_sec: u64) -> Profile {
    Profile {
        id: id.to_string(),
        name: format!("Test {}", id),
        keywords: vec![],
        negative_keywords: vec![],
        sources: vec!["ebay".to_string()],
        poll_interval_sec: interval_sec,
        price_min: None,
        price_max: None,
        enabled: true,
    }
}

fn noop_callback() -> PollCallback {
    Arc::new(|_p| Box::pin(async { vec![] }))
}

fn counting_callback(counter: &Arc<AtomicU32>) -> PollCallback {
    let c = counter.clone();
    Arc::new(move |_p| {
        let c = c.clone();
        Box::pin(async move {
            c.fetch_add(1, Ordering::Relaxed);
            vec![]
        })
    })
}

#[tokio::test]
async fn add_profile_creates_job() {
    let scheduler = PollScheduler::new();
    scheduler.start().await;

    let profile = test_profile("test-1", 60);
    scheduler
        .add_profile(profile, noop_callback(), None)
        .await;

    assert!(scheduler.has_job("test-1").await);
    scheduler.stop().await;
}

#[tokio::test]
async fn remove_profile_cancels_job() {
    let scheduler = PollScheduler::new();
    scheduler.start().await;

    let profile = test_profile("test-2", 60);
    scheduler
        .add_profile(profile, noop_callback(), None)
        .await;
    assert!(scheduler.has_job("test-2").await);

    scheduler.remove_profile("test-2").await;
    assert!(!scheduler.has_job("test-2").await);
    scheduler.stop().await;
}

#[tokio::test]
async fn disabled_profile_not_added() {
    let scheduler = PollScheduler::new();
    scheduler.start().await;

    let mut profile = test_profile("disabled-1", 60);
    profile.enabled = false;
    scheduler
        .add_profile(profile, noop_callback(), None)
        .await;

    assert!(!scheduler.has_job("disabled-1").await);
    scheduler.stop().await;
}

#[tokio::test]
async fn trigger_now_fires_callback() {
    let counter = Arc::new(AtomicU32::new(0));
    let callback = counting_callback(&counter);

    let scheduler = PollScheduler::new();
    scheduler.start().await;

    // Use a long interval so the timer doesn't fire during the test
    let profile = test_profile("trigger-1", 3600);
    scheduler.add_profile(profile, callback, None).await;

    // Wait briefly for the initial tick to fire (interval fires immediately)
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    let before_trigger = counter.load(Ordering::Relaxed);

    scheduler.trigger_now("trigger-1").await;
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let after_trigger = counter.load(Ordering::Relaxed);
    assert!(
        after_trigger > before_trigger,
        "trigger_now should have fired the callback (before={}, after={})",
        before_trigger,
        after_trigger
    );
    scheduler.stop().await;
}

#[tokio::test]
async fn jitter_delays_when_recently_polled() {
    let counter = Arc::new(AtomicU32::new(0));
    let callback = counting_callback(&counter);

    let scheduler = PollScheduler::new();
    scheduler.start().await;

    let profile = test_profile("jitter-1", 100);
    // Last polled just now -- next run should be delayed by ~100s
    scheduler
        .add_profile(profile, callback, Some(Utc::now()))
        .await;

    // The callback should NOT fire in 300ms because the interval hasn't elapsed
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
    assert_eq!(
        counter.load(Ordering::Relaxed),
        0,
        "callback should not fire when last_polled is recent"
    );

    scheduler.stop().await;
}

#[tokio::test]
async fn stop_cancels_all_jobs() {
    let scheduler = PollScheduler::new();
    scheduler.start().await;

    for i in 0..3 {
        let profile = test_profile(&format!("stop-{}", i), 60);
        scheduler
            .add_profile(profile, noop_callback(), None)
            .await;
    }

    scheduler.stop().await;

    assert!(!scheduler.has_job("stop-0").await);
    assert!(!scheduler.has_job("stop-1").await);
    assert!(!scheduler.has_job("stop-2").await);
}
