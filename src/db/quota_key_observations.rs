mod storage;

pub use storage::{
    EligibleObservationEndpoint, ObservationEndpoint, delete_expired_observations,
    eligible_endpoints, insert_observation, latest_observation, list_endpoint_observations,
    list_observations, observation_endpoint,
};

pub const OBSERVATION_RETENTION_DAYS: i64 = 30;
pub const OBSERVATION_PRUNE_BATCH_SIZE: i64 = 1_000;
pub const OBSERVATION_PAGE_MAX: i64 = 200;

#[cfg(test)]
mod tests;
