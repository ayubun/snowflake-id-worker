use snowflake_id_worker::run_worker;
use tokio::signal;

#[tokio::main]
async fn main() {
    let handle = tokio::spawn(async {
        run_worker().await;
    });

    match signal::ctrl_c().await {
        Ok(()) => {},
        Err(err) => {
            eprintln!("Unable to listen for shutdown signal: {}", err);
            // we also shut down in case of error
        },
    }
    handle.abort();
}
