use clap::Parser;
use snowflake::SnowflakeIdGenerator;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use warp::Filter;

const MAX_DATA_CENTER_ID: u8 = (1 << 5) - 1;
const MAX_WORKER_ID: u8 = (1 << 5) - 1;

const DEFAULT_EPOCH: SystemTime = UNIX_EPOCH;

#[derive(serde::Deserialize)]
struct GenerateRequest {
    count: Option<i64>,
}

#[derive(Debug, clap::Parser)]
struct Args {
    // TO SET WORKER ID AUTOMATICALLY IN A K8S STATEFUL SET, SET TO "FROM_HOSTNAME"
    #[arg(long, default_value = "0")]
    worker_id: String,

    #[arg(long, default_value = "0")]
    data_center_id: u8,

    #[arg(long)]
    epoch: Option<u64>,
}

pub fn create_routes() -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    // NOTE(ayubun): I'm not certain if Arc<Mutex<SnowflakeIdGenerator>> is the best way to go
    // about this, so if any onlookers have a more clever idea, please open a pull request or issue <3
    let snowflake_generator: Arc<Mutex<SnowflakeIdGenerator>> =
        Arc::new(Mutex::new(snowflake_id_generator_from_env()));

    // Optional `GET /health` endpoint for health checks
    let health_api = warp::path!("health").and(warp::get()).map(|| "OK");

    // `POST /generate` endpoint ヽ(*・ω・)ﾉ
    let generate_api = warp::path!("generate")
        .and(warp::post())
        .and(warp::body::bytes())
        .map(move |body: warp::hyper::body::Bytes| {
            // NOTE(ayubun): We parse JSON manually to handle malformed JSON as a 400 Bad Request.
            // This decision was made because the default behaviour is to silently fallback to the
            // empty body route, which generates 1 ID. I feel like this isn't as ergonomic as the
            // API telling you that you've made an error loudly so that you can fix it.
            let request: Option<GenerateRequest> = if body.is_empty() {
                None
            } else {
                match serde_json::from_slice(&body) {
                    Ok(req) => Some(req),
                    Err(_) => {
                        return warp::reply::with_status(
                            "Invalid JSON format".to_string(),
                            warp::http::StatusCode::BAD_REQUEST,
                        );
                    }
                }
            };

            let count = request.and_then(|r| r.count).unwrap_or(1);

            // NOTE(ayubun): We want to also return a 400 Bad Request for zero or negative count
            // for similar reasons to the JSON parsing.
            if count <= 0 {
                return warp::reply::with_status(
                    "Invalid count: must be a positive integer".to_string(),
                    warp::http::StatusCode::BAD_REQUEST,
                );
            }

            let mut ids: Vec<i64> = Vec::with_capacity(count as usize);
            let mut unlocked_generator = snowflake_generator.lock().unwrap();
            for _ in 0..count {
                ids.push(unlocked_generator.real_time_generate());
            }
            let response = format!(
                "[{}]",
                ids.into_iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            );
            warp::reply::with_status(response, warp::http::StatusCode::OK)
        });

    // TODO(ayubun): Add support for GRPC ? :3
    generate_api.or(health_api)
}

fn snowflake_id_generator_from_env() -> SnowflakeIdGenerator {
    let args = Args::parse();

    let epoch: SystemTime = args
        .epoch
        .map(|e| UNIX_EPOCH + Duration::from_millis(e))
        .unwrap_or(DEFAULT_EPOCH);

    let worker_id = if args.worker_id.eq_ignore_ascii_case("FROM_HOSTNAME") {
        // NOTE(ayubun): assuming this is being run from a stateful set in k8s:
        //
        // snowflake-id-worker-0
        // snowflake-id-worker-1
        // ...
        // snowflake-id-worker-n
        //
        // this code will try to grab the pod's index (n) and use it as the worker id
        hostname::get()
            .map(|os| os.to_string_lossy().into_owned())
            .expect("cannot retrieve hostname (WORKER_ID is being parsed from hostname)")
            .rsplit_once('-')
            .expect(
                "cannot split WORKER_ID from hostname (WORKER_ID is being parsed from hostname)",
            )
            .1
            .parse::<u8>()
            .expect(
                "cannot parse WORKER_ID from hostname (WORKER_ID is being parsed from hostname)",
            )
    } else {
        args.worker_id.parse::<u8>().unwrap_or_else(|_| {
            panic!(
                "cannot parse WORKER_ID as a valid u8 (WORKER_ID: \"{}\")",
                args.worker_id
            )
        })
    };

    if args.data_center_id > MAX_DATA_CENTER_ID {
        panic!("DATA_CENTER_ID must be less than {MAX_DATA_CENTER_ID}");
    }

    if worker_id > MAX_WORKER_ID {
        panic!("WORKER_ID must be less than {MAX_WORKER_ID}");
    }

    println!("starting snowflake-id-worker with WORKER_ID: {worker_id}, DATA_CENTER_ID: {}, and EPOCH: {epoch:?}", args.data_center_id);

    SnowflakeIdGenerator::with_epoch(args.data_center_id as i32, worker_id as i32, epoch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashSet;

    use warp::test::request;

    #[tokio::test]
    async fn test_health_endpoint() {
        let routes = create_routes();

        let resp = request().method("GET").path("/health").reply(&routes).await;

        assert_eq!(resp.status(), 200);
        assert_eq!(resp.body(), "OK");
    }

    #[tokio::test]
    async fn test_generate_endpoint_no_payload() {
        let routes = create_routes();

        let resp = request()
            .method("POST")
            .path("/generate")
            .reply(&routes)
            .await;

        assert_eq!(resp.status(), 200);
        let body = std::str::from_utf8(resp.body()).unwrap();

        assert!(body.starts_with("[") && body.ends_with("]"));

        let ids: Vec<i64> = serde_json::from_str(body).unwrap();
        assert_eq!(ids.len(), 1);
        assert!(ids[0] > 0);
    }

    #[tokio::test]
    async fn test_generate_endpoint_with_count() {
        let routes = create_routes();

        let payload = json!({"count": 10});

        let resp = request()
            .method("POST")
            .path("/generate")
            .json(&payload)
            .reply(&routes)
            .await;

        assert_eq!(resp.status(), 200);
        let body = std::str::from_utf8(resp.body()).unwrap();

        let ids: Vec<i64> = serde_json::from_str(body).unwrap();
        assert_eq!(ids.len(), 10);

        let unique_ids: HashSet<i64> = ids.iter().cloned().collect();
        assert_eq!(unique_ids.len(), 10, "All IDs should be unique");
        assert!(ids.iter().all(|&id| id > 0), "All IDs should be positive");
    }

    #[tokio::test]
    async fn test_generate_endpoint_with_large_count() {
        let routes = create_routes();

        let payload = json!({"count": 100000});

        let resp = request()
            .method("POST")
            .path("/generate")
            .json(&payload)
            .reply(&routes)
            .await;

        assert_eq!(resp.status(), 200);
        let body = std::str::from_utf8(resp.body()).unwrap();

        let ids: Vec<i64> = serde_json::from_str(body).unwrap();
        assert_eq!(ids.len(), 100000);

        let unique_ids: HashSet<i64> = ids.iter().cloned().collect();
        assert_eq!(unique_ids.len(), 100000, "All IDs should be unique");
    }

    #[tokio::test]
    async fn test_concurrent_http_requests() {
        let routes = create_routes();

        let num_requests = 50;
        let ids_per_request = 10;

        let mut handles = Vec::new();
        for _ in 0..num_requests {
            let routes_clone = routes.clone();
            let handle = tokio::spawn(async move {
                let payload = json!({"count": ids_per_request});
                let resp = request()
                    .method("POST")
                    .path("/generate")
                    .json(&payload)
                    .reply(&routes_clone)
                    .await;

                assert_eq!(resp.status(), 200);
                let body = std::str::from_utf8(resp.body()).unwrap();
                let ids: Vec<i64> = serde_json::from_str(body).unwrap();
                assert_eq!(ids.len(), ids_per_request);

                ids
            });
            handles.push(handle);
        }

        let mut all_ids = Vec::new();
        for handle in handles {
            let ids = handle.await.unwrap();
            all_ids.extend(ids);
        }

        let total_ids = all_ids.len();
        let unique_ids: HashSet<i64> = all_ids.iter().cloned().collect();
        assert_eq!(
            unique_ids.len(),
            total_ids,
            "All IDs from concurrent requests should be unique"
        );
    }

    #[tokio::test]
    async fn test_invalid_request_methods() {
        let routes = create_routes();

        // /generate expects POST
        let resp = request()
            .method("GET")
            .path("/generate")
            .reply(&routes)
            .await;

        assert_eq!(resp.status(), 405); // Method Not Allowed

        // /health expects GET
        let resp = request()
            .method("POST")
            .path("/health")
            .reply(&routes)
            .await;

        assert_eq!(resp.status(), 405); // Method Not Allowed
    }

    #[tokio::test]
    async fn test_non_existent_endpoints() {
        let routes = create_routes();

        let resp = request()
            .method("GET")
            .path("/nonexistent")
            .reply(&routes)
            .await;

        assert_eq!(resp.status(), 404); // i bet u know this one ( ˙꒳˙ )
    }

    #[tokio::test]
    async fn test_payload_edge_cases() {
        let routes = create_routes();

        // validate that bad payloads return 400 Bad Request

        let payload = json!({"count": 0});
        let resp = request()
            .method("POST")
            .path("/generate")
            .json(&payload)
            .reply(&routes)
            .await;
        assert_eq!(resp.status(), 400);
        let body = std::str::from_utf8(resp.body()).unwrap();
        assert!(
            body.contains("Invalid count"),
            "Should contain error message about invalid count"
        );

        let payload = json!({"count": -5});
        let resp = request()
            .method("POST")
            .path("/generate")
            .json(&payload)
            .reply(&routes)
            .await;
        assert_eq!(resp.status(), 400);
        let body = std::str::from_utf8(resp.body()).unwrap();
        assert!(
            body.contains("Invalid count"),
            "Should contain error message about invalid count"
        );

        let resp = request()
            .method("POST")
            .path("/generate")
            .body(b"invalid json")
            .reply(&routes)
            .await;
        assert_eq!(resp.status(), 400);
        let body = std::str::from_utf8(resp.body()).unwrap();
        assert!(
            body.contains("Invalid JSON"),
            "Should contain error message about invalid JSON"
        );
    }
}
