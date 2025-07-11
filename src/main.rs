use snowflake_id_worker::run_worker;

#[tokio::main]
async fn main() {
    run_worker().await;
}
