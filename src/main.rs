use std::{
    env,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use warp::Filter;

use snowflake::SnowflakeIdGenerator;

const MAX_DATA_CENTER_ID: u8 = (1 << 5) - 1;
const MAX_WORKER_ID: u8 = (1 << 5) - 1;

const DEFAULT_EPOCH: SystemTime = UNIX_EPOCH;
const DEFAULT_DATA_CENTER_ID: u8 = 0;
const DEFAULT_WORKER_ID: u8 = 0;

#[derive(serde::Deserialize)]
struct GenerateRequest {
    count: Option<u64>,
}

#[tokio::main]
async fn main() {
    // I'm not certain if Arc<Mutex<SnowflakeIdGenerator>> is the best way to go about this,
    // so if any onlookers have a more clever idea, please open a pull request or issue <3
    let snowflake_generator: Arc<Mutex<SnowflakeIdGenerator>> =
        Arc::new(Mutex::new(snowflake_id_generator_from_env()));

    // Optional health check endpoint
    let health_api = warp::path!("health").and(warp::get()).map(|| "OK");

    // `POST /generate` endpoint ヽ(*・ω・)ﾉ
    let generate_api = warp::path!("generate")
        .and(warp::post())
        .and(
            warp::body::json()
                .map(Some)
                .or(warp::any().map(|| None))
                .unify(),
        )
        .map(move |request: Option<GenerateRequest>| {
            let count = request.and_then(|r| r.count).unwrap_or(1);
            let mut ids: Vec<i64> = Vec::with_capacity(count as usize);
            let mut unlocked_generator = snowflake_generator.lock().unwrap();
            for _ in 0..count {
                ids.push(unlocked_generator.real_time_generate());
            }
            format!(
                "[{}]",
                ids.into_iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        });

    // TODO(ayubun): Add support for GRPC ? :3

    warp::serve(generate_api.or(health_api))
        .run(([127, 0, 0, 1], 80))
        .await;
}

fn snowflake_id_generator_from_env() -> SnowflakeIdGenerator {
    let epoch: SystemTime = env::var("EPOCH")
        .map(|s| {
            UNIX_EPOCH + Duration::from_millis(s.parse::<u64>().expect("EPOCH must be a valid u64"))
        })
        .unwrap_or(DEFAULT_EPOCH);
    let data_center_id = env::var("DATA_CENTER_ID")
        .unwrap_or_else(|_| DEFAULT_DATA_CENTER_ID.to_string())
        .parse::<u8>()
        .expect("DATA_CENTER_ID must be a valid u8");
    let worker_id = env::var("WORKER_ID")
        .unwrap_or_else(|_| DEFAULT_WORKER_ID.to_string())
        .parse::<u8>()
        .expect("WORKER_ID must be a valid u8");

    if data_center_id > MAX_DATA_CENTER_ID {
        panic!("DATA_CENTER_ID must be less than {MAX_DATA_CENTER_ID}");
    }

    if worker_id > MAX_WORKER_ID {
        panic!("WORKER_ID must be less than {MAX_WORKER_ID}");
    }

    SnowflakeIdGenerator::with_epoch(data_center_id as i32, worker_id as i32, epoch)
}
