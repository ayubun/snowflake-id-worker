use snowflake_id_worker::{exit_signal, run_worker};

#[tokio::main]
async fn main() {
    let handle = tokio::spawn(async {
        run_worker().await;
    });

    match exit_signal().await {
        () => {},
        
    }
    handle.abort();
}
