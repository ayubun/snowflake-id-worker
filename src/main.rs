use clap::Parser;
use snowflake_id_worker::create_routes;

#[derive(Debug, clap::Parser)]
struct Args {
    #[arg(long, default_value = "80")]
    port: u16,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let routes = create_routes();
    warp::serve(routes).run(([0, 0, 0, 0], args.port)).await;
}
