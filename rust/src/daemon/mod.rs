pub mod scheduler;
pub mod socket;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::Notify;
use tracing::{debug, error, info, warn};

// tokio::sync::Mutex for async-held locks (db, plugins, config)
use tokio::sync::Mutex as AsyncMutex;
// std::sync::Mutex for short-lived locks readable from sync closures (status handler)
use std::sync::Mutex as SyncMutex;

use crate::ai::evaluator::{AIEvaluator, Evaluator, NoopEvaluator};
use crate::ai::models::AIConfig;
use crate::config::AppConfig;
use crate::db::Database;
use crate::models::{Listing, Profile};
use crate::plugins::{Plugin, PluginError};
use crate::plugins::ebay::EbayPlugin;
use crate::plugins::craigslist::CraigslistPlugin;
use crate::plugins::facebook::FacebookPlugin;
use crate::scoring::score_listing;

use self::scheduler::PollScheduler;
use self::socket::SocketServer;

pub struct Daemon {
    config: Arc<AsyncMutex<AppConfig>>,
    config_path: Option<PathBuf>,
    db: Arc<AsyncMutex<Database>>,
    scheduler: Arc<PollScheduler>,
    socket_server: Arc<SocketServer>,
    plugins: Arc<AsyncMutex<HashMap<String, Arc<dyn Plugin>>>>,
    evaluator: Arc<dyn Evaluator>,
    active_polls: Arc<SyncMutex<HashSet<(String, String)>>>,
    bot_blocks: Arc<SyncMutex<HashMap<String, String>>>,
    // Profile summary for the status handler — refreshed at startup and at the
    // end of every reload so status never has to touch the async config lock.
    profile_summary: Arc<SyncMutex<Vec<serde_json::Value>>>,
    shutdown: Arc<Notify>,
}

/// Recover a std::sync mutex guard even if a prior holder panicked while
/// holding it — poll paths must never propagate a poison panic.
fn lock_recover<T>(mutex: &SyncMutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// STATUS-PROTOCOL CONTRACT: data.profiles is a flat array of enabled
/// profile-id strings — identical to the Python daemon's
/// `[p.id for p in profiles if p.enabled]`. Not objects, not disabled ids.
fn profile_summary_json(config: &AppConfig) -> Vec<serde_json::Value> {
    config
        .profiles
        .iter()
        .filter(|p| p.enabled)
        .map(|p| serde_json::Value::String(p.id.clone()))
        .collect()
}

fn make_plugins(config: &AppConfig) -> HashMap<String, Arc<dyn Plugin>> {
    let home_zip = config.global_config.home_zip.clone();
    let mut plugins: HashMap<String, Arc<dyn Plugin>> = HashMap::new();
    plugins.insert("ebay".to_string(), Arc::new(EbayPlugin));
    plugins.insert(
        "craigslist".to_string(),
        Arc::new(CraigslistPlugin::new(None, home_zip)),
    );
    plugins.insert(
        "facebook".to_string(),
        Arc::new(FacebookPlugin::new(None, None)),
    );
    plugins
}

/// Most recent last_polled timestamp across a profile's sources, read from
/// db source state — used both at startup and on reload so re-added
/// profiles don't lose their schedule and instantly re-poll.
async fn last_polled_for(db: &AsyncMutex<Database>, profile: &Profile) -> Option<DateTime<Utc>> {
    let mut last_polled: Option<DateTime<Utc>> = None;
    let db = db.lock().await;
    for source_id in &profile.sources {
        if let Ok(Some(state)) = db.get_source_state(source_id) {
            if let Some(ref lp) = state.last_polled {
                if let Ok(ts) = lp.parse::<DateTime<Utc>>() {
                    if last_polled.is_none_or(|prev| ts > prev) {
                        last_polled = Some(ts);
                    }
                }
            }
        }
    }
    last_polled
}

impl Daemon {
    pub fn new(
        config: AppConfig,
        ai_config: Option<AIConfig>,
        config_path: Option<PathBuf>,
    ) -> Self {
        let socket_path = config.socket_path();
        let db_path = config.db_path();
        let plugins = make_plugins(&config);

        let evaluator: Arc<dyn Evaluator> = match ai_config {
            Some(c) if c.enabled => {
                info!("AI evaluator enabled: filter={}, escalation={}", c.filter_model, c.escalation_model);
                Arc::new(AIEvaluator::new(c))
            }
            _ => Arc::new(NoopEvaluator),
        };

        let db = Database::new(&db_path).expect("failed to open database");
        let profile_summary = profile_summary_json(&config);

        Self {
            config: Arc::new(AsyncMutex::new(config)),
            config_path,
            db: Arc::new(AsyncMutex::new(db)),
            scheduler: Arc::new(PollScheduler::new()),
            socket_server: Arc::new(SocketServer::new(socket_path)),
            plugins: Arc::new(AsyncMutex::new(plugins)),
            evaluator,
            active_polls: Arc::new(SyncMutex::new(HashSet::new())),
            bot_blocks: Arc::new(SyncMutex::new(HashMap::new())),
            profile_summary: Arc::new(SyncMutex::new(profile_summary)),
            shutdown: Arc::new(Notify::new()),
        }
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use tracing_subscriber::EnvFilter;
        tracing_subscriber::fmt()
            // The daemon logs to a file that the TUI log panel renders —
            // ANSI escapes there break ratatui's column math into garble.
            .with_ansi(false)
            .with_target(false)
            .with_timer(tracing_subscriber::fmt::time::time())
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                    // chromiumoxide's conn/handler modules log ERROR-level spam for
                    // every CDP event newer Chrome emits that its protocol defs don't
                    // know (untagged-enum deserialize failures) — hundreds per poll,
                    // harmless. Real fetch failures are logged by the plugin layer.
                    EnvFilter::new(
                        "info,chromiumoxide::conn=off,chromiumoxide::handler=off,\
                         chromiumoxide=warn,tungstenite=warn",
                    )
                }),
            )
            .init();

        self.start().await?;

        let sig_shutdown = self.shutdown.clone();
        tokio::spawn(async move {
            let mut sigint =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                    .expect("failed to register SIGINT handler");
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("failed to register SIGTERM handler");
            tokio::select! {
                _ = sigint.recv() => {}
                _ = sigterm.recv() => {}
            }
            sig_shutdown.notify_one();
        });

        self.shutdown.notified().await;
        self.shutdown().await;
        Ok(())
    }

    /// Initializes the database, starts the scheduler and socket server,
    /// and registers all enabled profiles. Split out of `run()` so tests
    /// can drive a real daemon (real socket handlers, real scheduler)
    /// without installing a process-global tracing subscriber or signal
    /// handlers, and without blocking forever on the shutdown notify.
    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        {
            let db = self.db.lock().await;
            db.init()?;
            db.migrate()?;
        }
        self.evaluator.start().await;
        self.evaluator.preflight().await;
        let ai = self.evaluator.health();
        if ai.enabled && !ai.healthy {
            error!("AI EVALUATION IS NOT WORKING: {}", ai.detail);
        } else {
            info!("AI evaluation: {}", ai.detail);
        }
        self.scheduler.start().await;

        self.register_socket_handlers().await;
        self.socket_server.start().await?;
        self.register_profiles().await;

        let config = self.config.lock().await;
        let enabled_count = config.profiles.iter().filter(|p| p.enabled).count();
        info!(profiles = enabled_count, "daemon started");
        Ok(())
    }

    async fn register_socket_handlers(&self) {
        // Status: read real active_polls, bot_blocks, and the cached profile summary.
        // The profile summary is refreshed at startup and at the end of every reload,
        // so this handler never needs to touch the async config lock and can never
        // fabricate an empty profile list under contention.
        let active = self.active_polls.clone();
        let bots = self.bot_blocks.clone();
        let profile_summary = self.profile_summary.clone();
        let evaluator = self.evaluator.clone();
        self.socket_server
            .register_status_handler(Arc::new(move || {
                // `active_polls` stays a flat source-id list (stable wire
                // contract for the source spinner); `polling` carries the
                // full (profile, source) pairs so the UI can name the
                // profile currently being scraped.
                let (active_polls, polling): (Vec<String>, Vec<serde_json::Value>) = {
                    let guard = lock_recover(&active);
                    let unique: HashSet<String> =
                        guard.iter().map(|(_, source_id)| source_id.clone()).collect();
                    let pairs: Vec<serde_json::Value> = guard
                        .iter()
                        .map(|(profile_id, source_id)| {
                            serde_json::json!({"profile": profile_id, "source": source_id})
                        })
                        .collect();
                    (unique.into_iter().collect(), pairs)
                };
                let bot_blocks: HashMap<String, String> = lock_recover(&bots).clone();
                let profiles: Vec<serde_json::Value> = lock_recover(&profile_summary).clone();
                let ai = evaluator.health();

                serde_json::json!({
                    "state": "running",
                    "active_polls": active_polls,
                    "polling": polling,
                    "profiles": profiles,
                    "bot_blocks": bot_blocks,
                    "ai": {
                        "enabled": ai.enabled,
                        "healthy": ai.healthy,
                        "detail": ai.detail,
                    },
                })
            }))
            .await;

        // Poll: trigger scheduler for a specific profile
        let scheduler = self.scheduler.clone();
        self.socket_server
            .register_poll_handler(Arc::new(move |pid: String| {
                let s = scheduler.clone();
                Box::pin(async move {
                    s.trigger_now(&pid).await;
                })
            }))
            .await;

        // Shutdown
        let shutdown = self.shutdown.clone();
        self.socket_server
            .register_shutdown_handler(Arc::new(move || {
                shutdown.notify_one();
            }))
            .await;

        // Reload: re-read config from disk and diff profiles
        let config = self.config.clone();
        let config_path = self.config_path.clone();
        let scheduler = self.scheduler.clone();
        let db = self.db.clone();
        let plugins = self.plugins.clone();
        let evaluator = self.evaluator.clone();
        let active_polls = self.active_polls.clone();
        let bot_blocks = self.bot_blocks.clone();
        let profile_summary = self.profile_summary.clone();
        self.socket_server
            .register_reload_handler(Arc::new(move || {
                let config = config.clone();
                let config_path = config_path.clone();
                let scheduler = scheduler.clone();
                let db = db.clone();
                let plugins = plugins.clone();
                let evaluator = evaluator.clone();
                let active_polls = active_polls.clone();
                let bot_blocks = bot_blocks.clone();
                let profile_summary = profile_summary.clone();
                Box::pin(async move {
                    let Some(ref path) = config_path else {
                        warn!("config reload: no config path set");
                        return Err("no config path set".to_string());
                    };
                    let new_config = match crate::config::load_config(path) {
                        Ok(c) => c,
                        Err(e) => {
                            error!("config reload failed: {e}");
                            return Err(format!("config reload failed: {e}"));
                        }
                    };

                    // Diff profiles: remove old, add new/changed
                    let old_config = config.lock().await;
                    let old_ids: HashSet<String> =
                        old_config.profiles.iter().map(|p| p.id.clone()).collect();
                    let new_ids: HashSet<String> =
                        new_config.profiles.iter().map(|p| p.id.clone()).collect();

                    // Remove profiles that no longer exist
                    for removed in old_ids.difference(&new_ids) {
                        scheduler.remove_profile(removed).await;
                        info!(profile = %removed, "removed profile");
                    }
                    drop(old_config);

                    // Update plugins with new config
                    *plugins.lock().await = make_plugins(&new_config);

                    // Add/update profiles — read last_polled from db source state the
                    // same way startup registration does, so a reload never resets a
                    // profile's schedule and triggers a simultaneous re-poll storm.
                    // A profile whose definition is byte-for-byte unchanged from the
                    // already-registered job is skipped entirely: re-registering it
                    // would restart its job task and recompute initial_delay against
                    // "now", instantly re-polling a profile that may already be
                    // mid-scrape.
                    for profile in &new_config.profiles {
                        if !profile.enabled {
                            scheduler.remove_profile(&profile.id).await;
                            continue;
                        }

                        if scheduler.job_matches(profile).await {
                            continue;
                        }

                        let last_polled = last_polled_for(&db, profile).await;

                        let db = db.clone();
                        let plugins = plugins.clone();
                        let evaluator = evaluator.clone();
                        let active_polls = active_polls.clone();
                        let bot_blocks = bot_blocks.clone();

                        let callback = Arc::new(move |p: Profile| {
                            let db = db.clone();
                            let plugins = plugins.clone();
                            let evaluator = evaluator.clone();
                            let active_polls = active_polls.clone();
                            let bot_blocks = bot_blocks.clone();
                            Box::pin(async move {
                                poll_profile(&p, &db, &plugins, &evaluator, &active_polls, &bot_blocks)
                                    .await
                            })
                                as std::pin::Pin<Box<dyn std::future::Future<Output = Vec<Listing>> + Send>>
                        });

                        scheduler.add_profile(profile.clone(), callback, last_polled).await;
                    }

                    *lock_recover(&profile_summary) = profile_summary_json(&new_config);
                    *config.lock().await = new_config;
                    info!("config reloaded");
                    Ok(())
                })
            }))
            .await;
    }

    async fn register_profiles(&self) {
        let config = self.config.lock().await;
        for profile in &config.profiles {
            if !profile.enabled {
                continue;
            }

            let last_polled = last_polled_for(&self.db, profile).await;

            let db = self.db.clone();
            let plugins = self.plugins.clone();
            let evaluator = self.evaluator.clone();
            let active_polls = self.active_polls.clone();
            let bot_blocks = self.bot_blocks.clone();

            let callback = Arc::new(move |p: Profile| {
                let db = db.clone();
                let plugins = plugins.clone();
                let evaluator = evaluator.clone();
                let active_polls = active_polls.clone();
                let bot_blocks = bot_blocks.clone();
                Box::pin(async move {
                    poll_profile(&p, &db, &plugins, &evaluator, &active_polls, &bot_blocks).await
                })
                    as std::pin::Pin<Box<dyn std::future::Future<Output = Vec<Listing>> + Send>>
            });

            self.scheduler
                .add_profile(profile.clone(), callback, last_polled)
                .await;
        }
    }

    pub async fn shutdown(&self) {
        self.scheduler.stop().await;
        self.evaluator.stop().await;
        self.socket_server.stop().await;
        // DB closes when dropped
        drop(self.db.lock().await);
        info!("daemon shut down");
    }

    /// Test-support: whether the scheduler currently has a registered job
    /// for the given profile id.
    #[doc(hidden)]
    pub async fn has_profile_job(&self, profile_id: &str) -> bool {
        self.scheduler.has_job(profile_id).await
    }
}

/// RAII claim on an (profile_id, source_id) active-poll slot.
///
/// Construction only succeeds if the slot wasn't already held — this is
/// the mechanism that stops a scheduler re-registration, a manual "poll
/// now", or a stacked trigger from double-scraping the same source
/// concurrently. The slot is always released on drop, regardless of which
/// branch the poll took (success, bot-block, DB error), so a single
/// SQLITE_BUSY can never leave a phantom "active poll" in status forever.
struct ActivePollClaim<'a> {
    active_polls: &'a SyncMutex<HashSet<(String, String)>>,
    key: (String, String),
}

impl<'a> ActivePollClaim<'a> {
    fn try_claim(
        active_polls: &'a SyncMutex<HashSet<(String, String)>>,
        key: (String, String),
    ) -> Option<Self> {
        if lock_recover(active_polls).insert(key.clone()) {
            Some(Self { active_polls, key })
        } else {
            None
        }
    }
}

impl Drop for ActivePollClaim<'_> {
    fn drop(&mut self) {
        lock_recover(self.active_polls).remove(&self.key);
    }
}

/// Polls every source of `profile`. Never returns Err: a per-source
/// failure (DB error, bot detection, plugin error) is logged and handled
/// in place, and the loop always continues to the next source — a single
/// SQLITE_BUSY reading or writing source state can no longer abandon the
/// rest of the profile's sources or leave their active-poll slot stuck.
async fn poll_profile(
    profile: &Profile,
    db: &AsyncMutex<Database>,
    plugins: &AsyncMutex<HashMap<String, Arc<dyn Plugin>>>,
    evaluator: &Arc<dyn Evaluator>,
    active_polls: &SyncMutex<HashSet<(String, String)>>,
    bot_blocks: &SyncMutex<HashMap<String, String>>,
) -> Vec<Listing> {
    let mut new_listings = Vec::new();

    for source_id in &profile.sources {
        // Honor an active cooldown: after a bot block we back off this
        // source (exponentially, capped) rather than re-poking a site that
        // just challenged us — the single biggest lever for not getting
        // blocked again. The ⚠ indicator and error count are left as-is.
        {
            let db = db.lock().await;
            if let Ok(Some(state)) = db.get_source_state(source_id) {
                if let Some(until) = state
                    .rate_limit_until
                    .as_deref()
                    .and_then(|s| s.parse::<DateTime<Utc>>().ok())
                {
                    let remaining = until - Utc::now();
                    if remaining > chrono::Duration::zero() {
                        info!(
                            profile = %profile.id,
                            source = %source_id,
                            "cooling down after bot block, {}m left — skipping",
                            remaining.num_minutes()
                        );
                        continue;
                    }
                }
            }
        }

        let key = (profile.id.clone(), source_id.clone());
        let _claim = match ActivePollClaim::try_claim(active_polls, key) {
            Some(claim) => claim,
            None => {
                warn!(
                    profile = %profile.id,
                    source = %source_id,
                    "poll already in progress for this source, skipping"
                );
                continue;
            }
        };

        let result = poll_source(profile, source_id, db, plugins, evaluator.as_ref()).await;

        match result {
            Ok(listings) => {
                new_listings.extend(listings);
                {
                    let db = db.lock().await;
                    if let Err(e) = db.update_source_state(source_id, Some(Utc::now()), 0, None) {
                        error!(
                            profile = %profile.id,
                            source = %source_id,
                            error = %e,
                            "failed to record source poll state"
                        );
                    }
                }
                lock_recover(bot_blocks).remove(source_id);
            }
            Err(e) => {
                if let Some(PluginError::BotDetected { plugin_id, url, message }) = e.downcast_ref::<PluginError>() {
                    let db = db.lock().await;
                    let prior = db
                        .get_source_state(source_id)
                        .ok()
                        .flatten()
                        .map(|s| s.consecutive_errors)
                        .unwrap_or(0);
                    let errors = prior + 1;
                    let cooldown = bot_block_cooldown(errors);
                    let until = Utc::now() + cooldown;
                    warn!(
                        plugin = %plugin_id,
                        url = %url,
                        "bot block detected: {message} — backing off {}m (strike {errors})",
                        cooldown.num_minutes()
                    );
                    if let Err(e) =
                        db.update_source_state(source_id, None, errors, Some(until))
                    {
                        error!(source = %source_id, error = %e, "failed to record bot-block backoff");
                    }
                    lock_recover(bot_blocks).insert(plugin_id.clone(), url.clone());
                } else {
                    error!(
                        profile = %profile.id,
                        source = %source_id,
                        error = %e,
                        "poll failed"
                    );
                    let db = db.lock().await;
                    match db.get_source_state(source_id) {
                        Ok(state) => {
                            let current_errors =
                                state.as_ref().map(|s| s.consecutive_errors).unwrap_or(0);
                            let existing_last_polled = state.as_ref().and_then(|s| {
                                s.last_polled.as_ref().and_then(|lp| lp.parse::<DateTime<Utc>>().ok())
                            });
                            if let Err(e) = db.update_source_state(
                                source_id,
                                existing_last_polled,
                                current_errors + 1,
                                None,
                            ) {
                                error!(
                                    profile = %profile.id,
                                    source = %source_id,
                                    error = %e,
                                    "failed to record source error state"
                                );
                            }
                        }
                        Err(e) => {
                            error!(
                                profile = %profile.id,
                                source = %source_id,
                                error = %e,
                                "failed to read source state"
                            );
                        }
                    }
                }
            }
        }
    }

    new_listings
}

/// Minimum spacing between fetches of the same source across ALL profiles.
/// Many profiles share each marketplace; simultaneous searches from one
/// browser session read as automation — a 10-profile cold start produced
/// nine concurrent eBay hits (hard block) and identical generic Facebook
/// results for every query (soft flag).
/// Randomized spacing between same-source fetches. A jittered gap avoids
/// the clockwork-regular cadence that itself reads as automation; in tests
/// it collapses to zero so nothing waits on wall-clock.
fn source_fetch_gap() -> std::time::Duration {
    #[cfg(test)]
    {
        std::time::Duration::ZERO
    }
    #[cfg(not(test))]
    {
        use rand::Rng;
        std::time::Duration::from_secs(rand::thread_rng().gen_range(20..=45))
    }
}

type SourceGate = Arc<AsyncMutex<Option<std::time::Instant>>>;

static SOURCE_GATES: std::sync::LazyLock<SyncMutex<HashMap<String, SourceGate>>> =
    std::sync::LazyLock::new(|| SyncMutex::new(HashMap::new()));

fn source_gate(source_id: &str) -> SourceGate {
    lock_recover(&SOURCE_GATES)
        .entry(source_id.to_string())
        .or_default()
        .clone()
}

/// Exponential backoff after consecutive bot blocks: 15m, 30m, 1h, 2h, 4h,
/// capped at 6h. A successful poll resets the error count, collapsing the
/// ladder back to the first rung. `strikes` is the post-increment count
/// (>= 1).
fn bot_block_cooldown(strikes: i64) -> chrono::Duration {
    const BASE_MIN: i64 = 15;
    const CAP_MIN: i64 = 360;
    let exp = strikes.saturating_sub(1).min(20) as u32;
    let minutes = BASE_MIN.saturating_mul(2i64.saturating_pow(exp)).min(CAP_MIN);
    chrono::Duration::minutes(minutes)
}

async fn poll_source(
    profile: &Profile,
    source_id: &str,
    db: &AsyncMutex<Database>,
    plugins: &AsyncMutex<HashMap<String, Arc<dyn Plugin>>>,
    evaluator: &dyn Evaluator,
) -> Result<Vec<Listing>, Box<dyn std::error::Error + Send + Sync>> {
    // Clone the plugin Arc and drop the map lock before the (potentially
    // minutes-long) fetch — holding it across .await would block every other
    // profile's poll and block reload for the duration of one slow fetch.
    let plugin = {
        let plugins = plugins.lock().await;
        plugins
            .get(source_id)
            .cloned()
            .ok_or_else(|| format!("unknown plugin: {}", source_id))?
    };

    // One fetch per source at a time, with breathing room between them —
    // the gate is held across the fetch, so same-source polls from other
    // profiles queue here instead of stampeding the site.
    let gate = source_gate(source_id);
    let mut last_fetch = gate.lock().await;
    if let Some(prev) = *last_fetch {
        let since = prev.elapsed();
        let gap = source_fetch_gap();
        if since < gap {
            let wait = gap - since;
            debug!(source = source_id, "source gate: waiting {}s", wait.as_secs());
            tokio::time::sleep(wait).await;
        }
    }
    // Announce the fetch at INFO so the log tracks what the status bar
    // shows. Facebook in particular scrapes for tens of seconds without
    // logging anything until it finishes, which made the log look frozen
    // on the previous profile while the current poll was already underway.
    info!(profile = %profile.id, source = %source_id, "polling");
    let fetch_result = plugin.fetch(profile).await;
    *last_fetch = Some(std::time::Instant::now());
    drop(last_fetch);
    let fetched = fetch_result?;

    let ids: Vec<String> = fetched.iter().map(|l| l.id.clone()).collect();
    let known_ids = {
        let db = db.lock().await;
        db.get_existing_ids(&ids)?
    };

    let mut scored: Vec<Listing> = Vec::new();
    for mut listing in fetched {
        if known_ids.contains(&listing.id) {
            continue;
        }
        listing.relevance_score =
            score_listing(profile, &listing.title, &listing.description, listing.price);
        if listing.relevance_score > 0.0 {
            scored.push(listing);
        }
    }

    let evaluations = evaluator.evaluate_batch(profile, &scored).await;

    let scored_count = scored.len();
    let mut dropped = 0usize;
    let mut new_listings = Vec::new();
    let db = db.lock().await;
    for mut listing in scored {
        if let Some(eval) = evaluations.get(&listing.id) {
            if !eval.relevant {
                debug!(
                    title = %listing.title,
                    reason = %eval.reason,
                    "AI dropped listing"
                );
                dropped += 1;
                continue;
            }
            listing.ai_evaluation = Some(serde_json::to_string(eval)?);
        }
        if db.upsert_listing(&listing)? {
            new_listings.push(listing);
        }
    }
    if dropped > 0 {
        info!(
            "AI filtered {dropped}/{scored_count} listings for {}",
            profile.id
        );
    }

    Ok(new_listings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GlobalConfig;
    use crate::models::{AlertPriority, KeywordGroup};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn make_profile(id: &str, sources: &[&str]) -> Profile {
        make_profile_enabled(id, sources, true)
    }

    fn make_profile_enabled(id: &str, sources: &[&str], enabled: bool) -> Profile {
        Profile {
            id: id.to_string(),
            name: id.to_string(),
            keywords: vec![KeywordGroup::Single("x".into())],
            negative_keywords: vec![],
            sources: sources.iter().map(|s| s.to_string()).collect(),
            price_min: None,
            price_max: None,
            poll_interval_sec: 3600,
            alert_priority: AlertPriority::Normal,
            enabled,
            tags: vec![],
            escalation_keywords: vec![],
            location_radius_mi: None,
        }
    }

    fn open_db() -> Database {
        let db = Database::open(":memory:").unwrap();
        db.init().unwrap();
        db.migrate().unwrap();
        db
    }

    #[tokio::test]
    async fn last_polled_for_takes_most_recent_across_sources() {
        let db = open_db();
        let older = Utc::now() - chrono::Duration::hours(2);
        let newer = Utc::now() - chrono::Duration::minutes(5);
        db.update_source_state("ebay", Some(older), 0, None).unwrap();
        db.update_source_state("craigslist", Some(newer), 0, None).unwrap();
        let db = AsyncMutex::new(db);

        let profile = make_profile("p1", &["ebay", "craigslist"]);
        let result = last_polled_for(&db, &profile).await;

        assert_eq!(result, Some(newer));
    }

    #[tokio::test]
    async fn last_polled_for_none_when_source_never_polled() {
        let db = AsyncMutex::new(open_db());
        let profile = make_profile("p1", &["ebay"]);

        assert_eq!(last_polled_for(&db, &profile).await, None);
    }

    #[test]
    fn profile_summary_json_is_flat_array_of_enabled_profile_ids() {
        // STATUS-PROTOCOL CONTRACT: flat array of enabled profile-id
        // strings, disabled profiles excluded, no objects.
        let config = AppConfig {
            global_config: GlobalConfig::default(),
            profiles: vec![
                make_profile_enabled("p1", &["ebay"], true),
                make_profile_enabled("p2", &["craigslist"], false),
            ],
        };
        let summary = profile_summary_json(&config);
        assert_eq!(summary, vec![serde_json::Value::String("p1".to_string())]);

        let empty = AppConfig {
            global_config: GlobalConfig::default(),
            profiles: vec![],
        };
        assert!(profile_summary_json(&empty).is_empty());
    }

    #[test]
    fn lock_recover_yields_last_good_state_after_poison() {
        let mutex = SyncMutex::new(vec![1, 2, 3]);
        let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = mutex.lock().unwrap();
            panic!("simulated poison while holding the lock");
        }));
        assert!(poisoned.is_err());

        // A poisoned lock must not panic status/poll paths or fabricate empty data.
        let guard = lock_recover(&mutex);
        assert_eq!(*guard, vec![1, 2, 3]);
    }

    /// A plugin whose fetch parks on a Notify until released, counting how
    /// many times it was actually invoked — lets tests observe exactly
    /// when a fetch starts and prove claim semantics around it.
    struct SlowPlugin {
        started: Arc<Notify>,
        release: Arc<Notify>,
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl Plugin for SlowPlugin {
        fn plugin_id(&self) -> &str {
            "slow"
        }

        async fn fetch(
            &self,
            _profile: &Profile,
        ) -> Result<Vec<Listing>, Box<dyn std::error::Error + Send + Sync>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.started.notify_one();
            self.release.notified().await;
            Ok(vec![])
        }

        async fn supports_geo(&self) -> bool {
            false
        }
    }

    type SlowPluginFixture =
        (Arc<Notify>, Arc<Notify>, Arc<AtomicUsize>, Arc<AsyncMutex<HashMap<String, Arc<dyn Plugin>>>>);

    fn slow_plugin_fixture() -> SlowPluginFixture {
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let plugin: Arc<dyn Plugin> = Arc::new(SlowPlugin {
            started: started.clone(),
            release: release.clone(),
            calls: calls.clone(),
        });
        let mut map: HashMap<String, Arc<dyn Plugin>> = HashMap::new();
        map.insert("slow".to_string(), plugin);
        (started, release, calls, Arc::new(AsyncMutex::new(map)))
    }

    // F6b: while a fetch is parked, poll_source must have already dropped
    // the plugins-map lock (otherwise every other profile's poll would
    // stall behind one slow fetch), and the real active_polls set —
    // populated by poll_profile itself, not hand-built — must show the
    // source as in-flight while parked and be empty once it completes.
    #[tokio::test]
    async fn poll_profile_releases_plugins_lock_during_fetch_and_clears_active_polls_after() {
        let (started, release, calls, plugins) = slow_plugin_fixture();
        let db = Arc::new(AsyncMutex::new(open_db()));
        let evaluator: Arc<dyn Evaluator> = Arc::new(NoopEvaluator);
        let active_polls: Arc<SyncMutex<HashSet<(String, String)>>> =
            Arc::new(SyncMutex::new(HashSet::new()));
        let bot_blocks: Arc<SyncMutex<HashMap<String, String>>> =
            Arc::new(SyncMutex::new(HashMap::new()));
        let profile = make_profile("p1", &["slow"]);

        let handle = tokio::spawn({
            let (db, plugins, evaluator, active_polls, bot_blocks, profile) = (
                db.clone(),
                plugins.clone(),
                evaluator.clone(),
                active_polls.clone(),
                bot_blocks.clone(),
                profile.clone(),
            );
            async move {
                poll_profile(&profile, &db, &plugins, &evaluator, &active_polls, &bot_blocks).await
            }
        });

        started.notified().await;

        assert!(
            plugins.try_lock().is_ok(),
            "plugins map lock must be released before awaiting a slow fetch"
        );
        assert!(
            lock_recover(&active_polls).contains(&("p1".to_string(), "slow".to_string())),
            "active_polls must show the source in-flight while its fetch is parked"
        );

        release.notify_one();
        let listings = handle.await.unwrap();

        assert!(listings.is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(
            lock_recover(&active_polls).is_empty(),
            "active_polls must be cleared once the poll completes"
        );
    }

    // F6c: a second poll_profile call for the same (profile, source) while
    // the first is still parked must see the slot already claimed and
    // skip the fetch entirely, rather than double-scraping.
    #[tokio::test]
    async fn poll_profile_second_call_for_same_source_skips_while_first_in_flight() {
        let (started, release, calls, plugins) = slow_plugin_fixture();
        let db = Arc::new(AsyncMutex::new(open_db()));
        let evaluator: Arc<dyn Evaluator> = Arc::new(NoopEvaluator);
        let active_polls: Arc<SyncMutex<HashSet<(String, String)>>> =
            Arc::new(SyncMutex::new(HashSet::new()));
        let bot_blocks: Arc<SyncMutex<HashMap<String, String>>> =
            Arc::new(SyncMutex::new(HashMap::new()));
        let profile = make_profile("p1", &["slow"]);

        let first = tokio::spawn({
            let (db, plugins, evaluator, active_polls, bot_blocks, profile) = (
                db.clone(),
                plugins.clone(),
                evaluator.clone(),
                active_polls.clone(),
                bot_blocks.clone(),
                profile.clone(),
            );
            async move {
                poll_profile(&profile, &db, &plugins, &evaluator, &active_polls, &bot_blocks).await
            }
        });

        started.notified().await;

        let second =
            poll_profile(&profile, &db, &plugins, &evaluator, &active_polls, &bot_blocks).await;
        assert!(second.is_empty(), "claimed source must be skipped, not re-fetched");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "second call must not have invoked fetch while the first was in flight"
        );

        release.notify_one();
        first.await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn bot_block_cooldown_climbs_then_caps() {
        assert_eq!(bot_block_cooldown(1), chrono::Duration::minutes(15));
        assert_eq!(bot_block_cooldown(2), chrono::Duration::minutes(30));
        assert_eq!(bot_block_cooldown(3), chrono::Duration::minutes(60));
        assert_eq!(bot_block_cooldown(4), chrono::Duration::minutes(120));
        assert_eq!(bot_block_cooldown(5), chrono::Duration::minutes(240));
        // Caps at 6h and never overflows on a runaway error count.
        assert_eq!(bot_block_cooldown(6), chrono::Duration::minutes(360));
        assert_eq!(bot_block_cooldown(100), chrono::Duration::minutes(360));
    }

    #[tokio::test]
    async fn a_source_in_cooldown_is_skipped_without_fetching() {
        let (started, _release, calls, plugins) = slow_plugin_fixture();
        let db = Arc::new(AsyncMutex::new(open_db()));
        // Put the "slow" source in an active cooldown.
        {
            let db = db.lock().await;
            db.update_source_state("slow", None, 1, Some(Utc::now() + chrono::Duration::hours(1)))
                .unwrap();
        }
        let evaluator: Arc<dyn Evaluator> = Arc::new(NoopEvaluator);
        let active_polls = Arc::new(SyncMutex::new(HashSet::new()));
        let bot_blocks = Arc::new(SyncMutex::new(HashMap::new()));
        let profile = make_profile("p1", &["slow"]);

        let out = poll_profile(&profile, &db, &plugins, &evaluator, &active_polls, &bot_blocks).await;
        assert!(out.is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 0, "cooled-down source must not be fetched");
        // Nothing started, so the plugin's start-notify never fires.
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), started.notified())
                .await
                .is_err()
        );
    }

    /// A plugin that records how many fetches run concurrently.
    struct ConcurrencyProbe {
        current: Arc<AtomicUsize>,
        max_seen: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl Plugin for ConcurrencyProbe {
        fn plugin_id(&self) -> &str {
            "gate-probe"
        }

        async fn fetch(
            &self,
            _profile: &Profile,
        ) -> Result<Vec<Listing>, Box<dyn std::error::Error + Send + Sync>> {
            let now = self.current.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_seen.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            self.current.fetch_sub(1, Ordering::SeqCst);
            Ok(vec![])
        }

        async fn supports_geo(&self) -> bool {
            false
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn different_profiles_sharing_a_source_fetch_serially() {
        let current = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        let mut map: HashMap<String, Arc<dyn Plugin>> = HashMap::new();
        map.insert(
            "gate-probe".to_string(),
            Arc::new(ConcurrencyProbe {
                current: current.clone(),
                max_seen: max_seen.clone(),
            }),
        );
        let plugins = Arc::new(AsyncMutex::new(map));
        let db = Arc::new(AsyncMutex::new(open_db()));
        let evaluator: Arc<dyn Evaluator> = Arc::new(NoopEvaluator);
        let active_polls: Arc<SyncMutex<HashSet<(String, String)>>> =
            Arc::new(SyncMutex::new(HashSet::new()));
        let bot_blocks: Arc<SyncMutex<HashMap<String, String>>> =
            Arc::new(SyncMutex::new(HashMap::new()));

        // Nine distinct profiles all polling the same source at once — the
        // exact cold-start shape that got the session bot-flagged.
        let mut handles = Vec::new();
        for i in 0..9 {
            let profile = make_profile(&format!("p{i}"), &["gate-probe"]);
            let (db, plugins, evaluator, active_polls, bot_blocks) = (
                db.clone(),
                plugins.clone(),
                evaluator.clone(),
                active_polls.clone(),
                bot_blocks.clone(),
            );
            handles.push(tokio::spawn(async move {
                poll_profile(&profile, &db, &plugins, &evaluator, &active_polls, &bot_blocks)
                    .await
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        assert_eq!(
            max_seen.load(Ordering::SeqCst),
            1,
            "same-source fetches must never overlap across profiles"
        );
    }
}
