use std::{sync::Arc, time::Duration};

use sqlx::postgres::PgPoolOptions;
use tokio::{sync::oneshot, time::timeout};

use crate::{
    config::WorkerConfig, db::ConfigRepository, worker::runtime::lifecycle::RuntimeControl,
    worker_admin::chatgpt_quota_service::ChatGptQuotaService,
};

use super::{periodic_coordination_configured, run_periodic_pass_until_shutdown, spawn};

#[test]
fn periodic_collection_requires_a_valkey_coordination_url() {
    assert!(!periodic_coordination_configured(""));
    assert!(!periodic_coordination_configured("  "));
    assert!(periodic_coordination_configured("redis://127.0.0.1:6379"));
}

#[tokio::test]
async fn shutdown_cancels_a_running_coordinated_pass() {
    let control = RuntimeControl::new();
    let task_control = control.clone();
    let (started_tx, started_rx) = oneshot::channel();
    let pass = async move {
        let _ = started_tx.send(());
        std::future::pending::<
            Result<(), crate::worker_admin::chatgpt_quota_service::ChatGptQuotaError>,
        >()
        .await
    };
    let task =
        tokio::spawn(async move { run_periodic_pass_until_shutdown(&task_control, pass).await });

    started_rx.await.expect("scheduled pass started");
    control.begin_shutdown();
    assert_eq!(
        timeout(Duration::from_secs(1), task)
            .await
            .expect("shutdown cancels the running pass")
            .expect("pass task joins"),
        Ok(())
    );
}

#[tokio::test]
async fn shutdown_joins_the_request_wake_worker_without_periodic_coordination() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgresql://quota_test@127.0.0.1:55479/quota_759_test")
        .expect("lazy isolated test pool");
    let service = Arc::new(ChatGptQuotaService::new(
        pool.clone(),
        ConfigRepository::postgres(&pool),
    ));
    let mut config = WorkerConfig::default();
    config.valkey_url.clear();
    let control = RuntimeControl::new();
    let task = spawn(&config, service, control.clone());

    control.begin_shutdown();
    timeout(Duration::from_secs(1), task)
        .await
        .expect("collector shutdown is bounded")
        .expect("collector task joins");
}
