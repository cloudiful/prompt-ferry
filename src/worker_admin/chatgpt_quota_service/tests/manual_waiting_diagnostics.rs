#[test]
fn quota_failure_test_future_stays_bounded() {
    const MAX_FUTURE_SIZE_BYTES: usize = 64 * 1024;

    let root = async { super::manual_waiting_failure::failure_body().await };
    let body = super::manual_waiting_failure::failure_body();
    let root_size = std::mem::size_of_val(&root);
    let body_size = std::mem::size_of_val(&body);

    assert!(
        root_size <= MAX_FUTURE_SIZE_BYTES,
        "quota failure test root future is {root_size} bytes"
    );
    assert!(
        body_size <= MAX_FUTURE_SIZE_BYTES,
        "quota failure test body future is {body_size} bytes"
    );
}
