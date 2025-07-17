use snowflake_id_worker::{exit_signal, run_worker};

#[tokio::main]
async fn main() {
    tokio::select!(
        _ = exit_signal() => println!("Exiting from signal"),
        _ = run_worker() => println!("Worker exited"),
    )
}
