use snowflake_id_worker::run_worker;

#[tokio::main]
async fn main() {
    // graceful shutdown drains in-flight requests
    run_worker().await;
}
