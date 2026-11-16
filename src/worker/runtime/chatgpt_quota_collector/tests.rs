use std::time::Duration;

use tokio::{sync::oneshot, time::timeout};

use crate::worker::runtime::lifecycle::RuntimeControl;

use super::{periodic_coordination_configured, run_periodic_pass_until_shutdown};

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
async fn aborting_the_owner_cancels_the_composed_request_wakes_and_collector() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct DropProbe(Arc<AtomicUsize>);

    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    let dropped = Arc::new(AtomicUsize::new(0));
    let wakes_probe = dropped.clone();
    let periodic_probe = dropped.clone();
    let owner = tokio::spawn(async move {
        let request_wakes = async move {
            let _probe = DropProbe(wakes_probe);
            std::future::pending::<()>().await;
        };
        let periodic = async move {
            let _probe = DropProbe(periodic_probe);
            std::future::pending::<()>().await;
        };
        super::run_owned_worker(request_wakes, periodic).await;
    });

    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    owner.abort();

    assert!(owner.await.unwrap_err().is_cancelled());
    assert_eq!(
        dropped.load(Ordering::SeqCst),
        2,
        "aborting the owner drops both composed futures instead of detaching one"
    );
}
