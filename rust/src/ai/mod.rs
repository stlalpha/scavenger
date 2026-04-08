use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::models::{Listing, Profile};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIEvaluation {
    pub listing_id: String,
    pub relevant: bool,
    pub reason: Option<String>,
    pub escalated: bool,
}

type AsyncResult<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a>>;

/// Evaluator trait for AI batch evaluation of listings.
pub trait Evaluator: Send + Sync {
    fn start(&self) -> AsyncResult<'_, ()>;
    fn stop(&self) -> AsyncResult<'_, ()>;
    fn evaluate_batch(
        &self,
        profile: &Profile,
        listings: &[Listing],
    ) -> AsyncResult<'_, HashMap<String, AIEvaluation>>;
}

/// No-op evaluator: passes everything through unchanged.
pub struct NoopEvaluator;

impl Evaluator for NoopEvaluator {
    fn start(&self) -> AsyncResult<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn stop(&self) -> AsyncResult<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn evaluate_batch(
        &self,
        _profile: &Profile,
        _listings: &[Listing],
    ) -> AsyncResult<'_, HashMap<String, AIEvaluation>> {
        Box::pin(async { Ok(HashMap::new()) })
    }
}
