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
use crate::plugins::{Plugin, PluginError};
use crate::plugins::ebay::EbayPlugin;
use crate::plugins::craigslist::CraigslistPlugin;
use crate::plugins::facebook::FacebookPlugin;
use crate::scoring::score_listing;

use self::scheduler::PollScheduler;
use self::socket::SocketServer;

pub struct Daemon {
    config: Arc<Mutex<AppConfig>>,
    _config_path: Option<PathBuf>,
    db: Arc<Mutex<Database>>,
    scheduler: Arc<PollScheduler>,
    socket_server: Arc<SocketServer>,
    plugins: Arc<Mutex<HashMap<String, Box<dyn Plugin>>>>,
    evaluator: Arc<dyn Evaluator>,
    active_polls: Arc<Mutex<HashSet<String>>>,
    bot_blocks: Arc<Mutex<HashMap<String, String>>>,
    shutdown: Arc<Notify>,
}

fn make_plugins(config: &AppConfig) -> HashMap<String, Box<dyn Plugin>> {
    let home_zip = config.global_config.home_zip.clone();
    let mut plugins: HashMap<String, Box<dyn Plugin>> = HashMap::new();
    plugins.insert("ebay".to_string(), Box::new(EbayPlugin));
    plugins.insert(
        "craigslist".to_string(),
        Box::new(CraigslistPlugin::new(None, home_zip)),
    );
    plugins.insert(
        "facebook".to_string(),
        Box::new(FacebookPlugin::new(None, None)),
    );
    plugins
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

        let evaluator: Arc<dyn Evaluator> = if ai_config.as_ref().map_or(false, |c| c.enabled) {
            Arc::new(NoopEvaluator) // TODO: construct real AIEvaluator
        } else {
            Arc::new(NoopEvaluator)
        };

        let db = Database::new(&db_path).expect("failed to open database");

        Self {
            config: Arc::new(Mutex::new(config)),
            _config_path: config_path,
            db: Arc::new(Mutex::new(db)),
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
        tracing_subscriber::fmt()
            .with_target(false)
            .with_timer(tracing_subscriber::fmt::time::time())
            .init();

        {
            let db = self.db.lock().await;
            db.init()?;
            db.migrate()?;
        }
        self.evaluator.start().await;
        self.scheduler.start().await;

        self.register_socket_handlers().await;
        self.socket_server.start().await?;
        self.register_profiles().await;

        let config = self.config.lock().await;
        let enabled_count = config.profiles.iter().filter(|p| p.enabled).count();
        info!(
            profiles = enabled_count,
            "daemon started"
        );
        drop(config);

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

    async fn register_socket_handlers(&self) {
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

        let shutdown = self.shutdown.clone();
        self.socket_server
            .register_shutdown_handler(Arc::new(move || {
                shutdown.notify_one();
            }))
            .await;

        self.socket_server
            .register_reload_handler(Arc::new(|| {
                Box::pin(async { warn!("config reload not yet implemented") })
            }))
            .await;
    }

    async fn register_profiles(&self) {
        let config = self.config.lock().await;
        for profile in &config.profiles {
            if !profile.enabled {
                continue;
            }

            let mut last_polled: Option<DateTime<Utc>> = None;
            {
                let db = self.db.lock().await;
                for source_id in &profile.sources {
                    if let Ok(Some(state)) = db.get_source_state(source_id) {
                        if let Some(ref lp) = state.last_polled {
                            if let Ok(ts) = lp.parse::<DateTime<Utc>>() {
                                if last_polled.map_or(true, |prev| ts > prev) {
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
        self.evaluator.stop().await;
        self.socket_server.stop().await;
        // DB closes when dropped
        drop(self.db.lock().await);
        info!("daemon shut down");
    }
}

async fn poll_profile(
    profile: &Profile,
    db: &Mutex<Database>,
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
                {
                    let db = db.lock().await;
                    db.update_source_state(source_id, Some(Utc::now()), 0, None)?;
                }
                bot_blocks.lock().await.remove(source_id);
            }
            Err(e) => {
                if let Some(PluginError::BotDetected { plugin_id, url, message }) = e.downcast_ref::<PluginError>() {
                    warn!(
                        plugin = %plugin_id,
                        url = %url,
                        "bot block detected: {message}"
                    );
                    bot_blocks
                        .lock()
                        .await
                        .insert(plugin_id.clone(), url.clone());
                } else {
                    error!(
                        profile = %profile.id,
                        source = %source_id,
                        error = %e,
                        "poll failed"
                    );
                    let db = db.lock().await;
                    let state = db.get_source_state(source_id)?;
                    let current_errors = state.as_ref().map(|s| s.consecutive_errors).unwrap_or(0);
                    let existing_last_polled = state.as_ref().and_then(|s| {
                        s.last_polled.as_ref().and_then(|lp| lp.parse::<DateTime<Utc>>().ok())
                    });
                    db.update_source_state(
                        source_id,
                        existing_last_polled,
                        current_errors + 1,
                        None,
                    )?;
                }
            }
        }

        active_polls.lock().await.remove(source_id);
    }

    Ok(new_listings)
}

async fn poll_source(
    profile: &Profile,
    source_id: &str,
    db: &Mutex<Database>,
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

    let mut new_listings = Vec::new();
    let db = db.lock().await;
    for mut listing in scored {
        if let Some(eval) = evaluations.get(&listing.id) {
            if !eval.relevant {
                continue;
            }
            listing.ai_evaluation = Some(serde_json::to_string(eval)?);
        }
        if db.upsert_listing(&listing)? {
            new_listings.push(listing);
        }
    }

    Ok(new_listings)
}
