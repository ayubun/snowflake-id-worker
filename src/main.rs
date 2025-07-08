use snowflake_id_worker::create_routes;

#[tokio::main]
async fn main() {
    // NOTE(ayubun): The `PORT` env is not documented because the Dockerfile only exposes
    // port 80. Changing the port via env is mostly valuable for local development, where
    // port 80 might be taken by something else~
    let port = std::env::var("PORT")
        .unwrap_or("80".to_string())
        .parse::<u16>()
        .expect("PORT must be a valid u16");
    let routes = create_routes();
    warp::serve(routes).run(([0, 0, 0, 0], port)).await;
}
