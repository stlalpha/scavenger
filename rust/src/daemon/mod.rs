pub mod scheduler;
pub mod socket;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::{Mutex, Notify};
use tracing::{error, info, warn};

use crate::ai::evaluator::{AIEvaluator, Evaluator, NoopEvaluator};
use crate::ai::models::AIConfig;
use crate::config::AppConfig;
use crate::db::Database;
use crate::models::{Listing, Profile};
use crate::plugins::{BotDetectedError, Plugin};
use crate::scoring::score_listing;

use self::scheduler::PollScheduler;
use self::socket::SocketServer;

pub struct Daemon {
    config: Arc<Mutex<AppConfig>>,
    config_path: Option<PathBuf>,
    db: Arc<Database>,
    scheduler: Arc<PollScheduler>,
    socket_server: Arc<SocketServer>,
    plugins: Arc<Mutex<HashMap<String, Box<dyn Plugin>>>>,
    evaluator: Arc<dyn Evaluator>,
    active_polls: Arc<Mutex<HashSet<String>>>,
    bot_blocks: Arc<Mutex<HashMap<String, String>>>,
    shutdown: Arc<Notify>,
}

fn make_plugins(_config: &AppConfig) -> HashMap<String, Box<dyn Plugin>> {
    // Stub: actual plugins are implemented in their own work units.
    // In production this would instantiate EbayPlugin, CraigslistPlugin, etc.
    HashMap::new()
}

impl Daemon {
    pub fn new(
        config: AppConfig,
        ai_config: Option<AIConfig>,
        config_path: Option<PathBuf>,
    ) -> Self {
        let socket_path = config.socket_path.clone();
        let db_path = config.db_path.clone();
        let plugins = make_plugins(&config);

        let evaluator: Arc<dyn Evaluator> = if ai_config.as_ref().map_or(false, |c| c.enabled) {
            // Stub: real AIEvaluator would be constructed here
            Arc::new(NoopEvaluator)
        } else {
            Arc::new(NoopEvaluator)
        };

        Self {
            config: Arc::new(Mutex::new(config)),
            config_path,
            db: Arc::new(Database::new(db_path)),
            scheduler: Arc::new(PollScheduler::new()),
            socket_server: Arc::new(SocketServer::new(socket_path)),
            plugins: Arc::new(Mutex::new(plugins)),
            evaluator,
            active_polls: Arc::new(Mutex::new(HashSet::new())),
            bot_blocks: Arc::new(Mutex::new(HashMap::new())),
            shutdown: Arc::new(Notify::new()),
        }
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.db.init().await?;
        self.db.migrate().await?;
        self.evaluator.start().await?;
        self.scheduler.start().await;

        // Register socket handlers before starting the listener
        self.register_socket_handlers().await;
        self.socket_server.start().await?;
        self.register_profiles().await;

        let config = self.config.lock().await;
        let enabled_count = config.profiles.iter().filter(|p| p.enabled).count();
        info!(
            socket = %config.socket_path.display(),
            profiles = enabled_count,
            "daemon started"
        );
        drop(config);

        // Wait for shutdown signal (SIGINT/SIGTERM) or explicit notify
        let shutdown = self.shutdown.clone();
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

        shutdown.notified().await;
        self.shutdown().await;
        Ok(())
    }

    async fn register_socket_handlers(&self) {
        // Status handler (sync — can't await Mutex locks, returns static shape for now)
        self.socket_server
            .register_status_handler(Arc::new(|| {
                serde_json::json!({
                    "state": "running",
                    "active_polls": [],
                    "profiles": [],
                    "bot_blocks": {},
                })
            }))
            .await;

        let config_for_poll = self.config.clone();
        let db = self.db.clone();
        let plugins = self.plugins.clone();
        let evaluator = self.evaluator.clone();
        let active_polls_for_poll = self.active_polls.clone();
        let bot_blocks_for_poll = self.bot_blocks.clone();
        self.socket_server
            .register_poll_handler(Arc::new(move |profile_id: String| {
                let config = config_for_poll.clone();
                let db = db.clone();
                let plugins = plugins.clone();
                let evaluator = evaluator.clone();
                let active_polls = active_polls_for_poll.clone();
                let bot_blocks = bot_blocks_for_poll.clone();
                Box::pin(async move {
                    let cfg = config.lock().await;
                    let profile = cfg.profiles.iter().find(|p| p.id == profile_id).cloned();
                    drop(cfg);
                    if let Some(profile) = profile {
                        let _ =
                            poll_profile(&profile, &db, &plugins, &evaluator, &active_polls, &bot_blocks)
                                .await;
                    }
                })
            }))
            .await;

        // Reload handler (stub — full implementation will re-read config and diff profiles)
        self.socket_server
            .register_reload_handler(Arc::new(|| {
                Box::pin(async { warn!("config reload not yet implemented in Rust port") })
            }))
            .await;

        // Shutdown handler
        let shutdown = self.shutdown.clone();
        self.socket_server
            .register_shutdown_handler(Arc::new(move || {
                shutdown.notify_one();
            }))
            .await;
    }

    async fn register_profiles(&self) {
        let config = self.config.lock().await;
        for profile in &config.profiles {
            if !profile.enabled {
                continue;
            }

            // Find the most recent poll time across this profile's sources
            let mut last_polled: Option<DateTime<Utc>> = None;
            for source_id in &profile.sources {
                if let Ok(Some(state)) = self.db.get_source_state(source_id).await {
                    if let Some(ts_val) = state.get("last_polled") {
                        if let Some(ts_str) = ts_val.as_str() {
                            if let Ok(ts) = ts_str.parse::<DateTime<Utc>>() {
                                if last_polled.map_or(true, |lp| ts > lp) {
                                    last_polled = Some(ts);
                                }
                            }
                        }
                    }
                }
            }

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
                    let result =
                        poll_profile(&p, &db, &plugins, &evaluator, &active_polls, &bot_blocks)
                            .await;
                    result.unwrap_or_default()
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
        let _ = self.evaluator.stop().await;
        self.socket_server.stop().await;
        let _ = self.db.close().await;
        info!("daemon shut down");
    }
}

/// Execute a poll for a single profile across all its sources.
async fn poll_profile(
    profile: &Profile,
    db: &Database,
    plugins: &Mutex<HashMap<String, Box<dyn Plugin>>>,
    evaluator: &Arc<dyn Evaluator>,
    active_polls: &Mutex<HashSet<String>>,
    bot_blocks: &Mutex<HashMap<String, String>>,
) -> Result<Vec<Listing>, Box<dyn std::error::Error + Send + Sync>> {
    let mut new_listings = Vec::new();

    for source_id in &profile.sources {
        active_polls.lock().await.insert(source_id.clone());

        let result = poll_source(profile, source_id, db, plugins, evaluator.as_ref()).await;

        match result {
            Ok(listings) => {
                new_listings.extend(listings);
                db.update_source_state(source_id, Some(Utc::now()), None)
                    .await?;
                bot_blocks.lock().await.remove(source_id);
            }
            Err(e) => {
                // Check if it's a bot detection error
                if let Some(bot_err) = e.downcast_ref::<BotDetectedError>() {
                    warn!(
                        plugin = %bot_err.plugin_id,
                        url = %bot_err.url,
                        "bot block detected"
                    );
                    bot_blocks
                        .lock()
                        .await
                        .insert(bot_err.plugin_id.clone(), bot_err.url.clone());
                } else {
                    error!(
                        profile = %profile.id,
                        source = %source_id,
                        error = %e,
                        "poll failed"
                    );
                    // Increment consecutive errors
                    let state = db.get_source_state(source_id).await?;
                    let current_errors = state
                        .as_ref()
                        .and_then(|s| s.get("consecutive_errors"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32;
                    let existing_last_polled = state
                        .as_ref()
                        .and_then(|s| s.get("last_polled"))
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse::<DateTime<Utc>>().ok());
                    db.update_source_state(
                        source_id,
                        existing_last_polled,
                        Some(current_errors + 1),
                    )
                    .await?;
                }
            }
        }

        active_polls.lock().await.remove(source_id);
    }

    Ok(new_listings)
}

/// Poll a single source within a profile: fetch, dedup, score, AI eval, upsert.
async fn poll_source(
    profile: &Profile,
    source_id: &str,
    db: &Database,
    plugins: &Mutex<HashMap<String, Box<dyn Plugin>>>,
    evaluator: &dyn Evaluator,
) -> Result<Vec<Listing>, Box<dyn std::error::Error + Send + Sync>> {
    let fetched = {
        let plugins = plugins.lock().await;
        let plugin = plugins
            .get(source_id)
            .ok_or_else(|| format!("unknown plugin: {}", source_id))?;
        plugin.fetch(profile).await?
    };

    let ids: Vec<String> = fetched.iter().map(|l| l.id.clone()).collect();
    let known_ids = db.get_existing_ids(&ids).await?;

    let mut scored: Vec<Listing> = Vec::new();
    for mut listing in fetched {
        if known_ids.contains(&listing.id) {
            continue;
        }
        listing.relevance_score = score_listing(profile, &listing.title, &listing.description, listing.price);
        if listing.relevance_score > 0.0 {
            scored.push(listing);
        }
    }

    // Batch AI evaluation
    let evaluations = evaluator.evaluate_batch(profile, &scored).await?;

    let mut new_listings = Vec::new();
    for mut listing in scored {
        if let Some(eval) = evaluations.get(&listing.id) {
            if !eval.relevant {
                continue;
            }
            listing.ai_evaluation = Some(serde_json::to_string(eval)?);
        }
        if db.upsert_listing(&listing).await? {
            new_listings.push(listing);
        }
    }

    Ok(new_listings)
}
