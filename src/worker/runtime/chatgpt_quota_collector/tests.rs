use std::{sync::Arc, time::Duration};

use tokio::{sync::oneshot, time::timeout};

use crate::{
    config::WorkerConfig, db::ConfigRepository, worker::runtime::lifecycle::RuntimeControl,
    worker_admin::chatgpt_quota_service::ChatGptQuotaService,
};

use super::{periodic_coordination_configured, run_periodic_pass_until_shutdown, spawn};

#[allow(dead_code)]
mod db_harness {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/db_harness.rs"
    ));
}

/// The pure guard cases for the shared dedicated-test-target helper
/// (`tests/support/test_pg_target.rs`) live here because this file owns the
/// collector's target construction: they need no server and no environment, so
/// they pin the fail-closed contract before any pool is ever built.
mod target_guard {
    use super::db_harness::test_pg_target::{
        CONFIRMED_TARGET_KEY, DATABASE_URL_KEY, TestTarget, parse_target,
    };

    const PASSWORD: &str = "pg-fixture-secret";
    const URL: &str = "postgres://quota_tester:pg-fixture-secret@db.example.test:5433/postgres";
    const CONFIRMED: &str = "db.example.test:5433/postgres";

    fn target(url: &str, confirmed: &str) -> TestTarget {
        parse_target(url, confirmed).unwrap_or_else(|error| panic!("{error}"))
    }

    fn refusal(url: &str, confirmed: &str) -> String {
        parse_target(url, confirmed)
            .err()
            .map(|error| error.to_string())
            .unwrap_or_else(|| "the target was accepted".to_string())
    }

    #[test]
    fn an_exact_confirmed_target_is_accepted_with_its_credentials() {
        let accepted = target(URL, CONFIRMED);
        assert_eq!(accepted.identity(), CONFIRMED);
        let options = accepted.connect_options();
        assert_eq!(options.get_host(), "db.example.test");
        assert_eq!(options.get_port(), 5433);
        assert_eq!(options.get_database(), Some("postgres"));
        assert_eq!(options.get_username(), "quota_tester");
        assert!(accepted.has_password());
    }

    #[test]
    fn host_case_and_the_default_port_do_not_change_the_identity() {
        let defaulted = target(
            "postgresql://quota_tester@DB.Example.Test/postgres",
            "db.example.test:5432/postgres",
        );
        assert_eq!(defaulted.identity(), "db.example.test:5432/postgres");
        assert!(!defaulted.has_password());
    }

    #[test]
    fn a_confirmed_target_pins_the_host_port_and_database() {
        let cases = [
            (
                "another host",
                "postgres://quota_tester@other.example.test:5433/postgres",
                CONFIRMED,
            ),
            (
                "another port",
                "postgres://quota_tester@db.example.test:5436/postgres",
                CONFIRMED,
            ),
            (
                "another database",
                "postgres://quota_tester@db.example.test:5433/other",
                CONFIRMED,
            ),
            // A production database on the confirmed host is still another target.
            (
                "the application database",
                "postgres://quota_tester@db.example.test:5433/prompt_ferry",
                CONFIRMED,
            ),
        ];
        for (label, url, confirmed) in cases {
            assert!(
                refusal(url, confirmed).contains("instead of the confirmed target"),
                "{label} must be refused"
            );
        }
    }

    #[test]
    fn the_refusal_names_the_mismatch_without_echoing_the_url() {
        let message = refusal(URL, "db.example.test:5433/prompt_ferry");
        assert!(message.contains(DATABASE_URL_KEY));
        assert!(message.contains("db.example.test:5433/postgres"));
        assert!(!message.contains("quota_tester"));
        assert!(!message.contains(PASSWORD));
    }

    #[test]
    fn malformed_and_unconfirmed_targets_fail_closed() {
        let cases = [
            ("not a url", CONFIRMED),
            (
                "postgres://quota_tester@db.example.test:5433/postgres extra",
                CONFIRMED,
            ),
            (
                "mysql://quota_tester@db.example.test:5433/postgres",
                CONFIRMED,
            ),
            ("postgres://db.example.test:5433/postgres", CONFIRMED),
            ("postgres://quota_tester@db.example.test:5433/", CONFIRMED),
            (
                "postgres://quota_tester@db.example.test:5433/postgres?options=-csearch_path%3Dpublic",
                CONFIRMED,
            ),
            (
                "postgres://quota_tester@db.example.test:5433/postgres#fragment",
                CONFIRMED,
            ),
            (
                "postgres://quota_tester@db.example.test:not-a-port/postgres",
                CONFIRMED,
            ),
            ("postgres://quota_tester@[::1]:5433/postgres", CONFIRMED),
        ];
        for (url, confirmed) in cases {
            assert!(
                parse_target(url, confirmed).is_err(),
                "{url} must be refused"
            );
        }

        let identities = [
            "db.example.test:5433",
            "db.example.test/postgres/extra",
            ":5433/postgres",
            "db.example.test:0/postgres",
        ];
        for confirmed in identities {
            assert!(
                parse_target(URL, confirmed).is_err(),
                "{confirmed} must be refused"
            );
        }
    }

    #[test]
    fn percent_escapes_normalize_rather_than_smuggle_a_different_identity() {
        // Encoding is normalized on both sides, so a spelled-out host still
        // matches and a look-alike host still does not.
        assert_eq!(
            target(
                "postgres://quota_tester@db.example%2Etest:5433/postgres",
                CONFIRMED
            )
            .identity(),
            CONFIRMED
        );
        assert!(
            refusal(
                "postgres://quota_tester@db.example.test.evil.test:5433/postgres",
                CONFIRMED
            )
            .contains("instead of the confirmed target")
        );
        assert!(
            parse_target(
                "postgres://quota_tester@db%ZZexample.test:5433/postgres",
                CONFIRMED
            )
            .is_err()
        );
    }

    #[test]
    fn the_dedicated_keys_are_the_only_ones_the_guard_names() {
        assert_eq!(DATABASE_URL_KEY, "PROMPT_FERRY_QUOTA_TEST_DATABASE_URL");
        assert_eq!(
            CONFIRMED_TARGET_KEY,
            "PROMPT_FERRY_QUOTA_TEST_CONFIRMED_TARGET"
        );
    }
}

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
    // Only shutdown is asserted, so this never-connects pool is enough and keeps
    // the test away from any real database.
    let pool = db_harness::test_db_url::lazy_test_pool();
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
