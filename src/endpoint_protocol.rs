use std::time::Duration;

use reqwest::Client;

pub fn endpoint_protocol_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .expect("static reqwest client config is valid")
}
