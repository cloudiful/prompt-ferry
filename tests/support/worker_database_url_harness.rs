use std::env;

use crate::db_harness::{TEST_DATABASE_URL_ENV, TestSchema};

pub fn worker_database_url(schema: &TestSchema) -> anyhow::Result<String> {
    let database_url = env::var(TEST_DATABASE_URL_ENV)?;
    let search_path_option = format!("-csearch_path={}", schema.schema_name);
    let options = urlencoding::encode(&search_path_option);
    let join = if database_url.contains('?') { "&" } else { "?" };
    Ok(format!("{database_url}{join}options={options}"))
}
