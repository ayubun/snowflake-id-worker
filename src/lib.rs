use clap::Parser;
use snowflake::SnowflakeIdGenerator;
use std::{
    env,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use warp::{http::StatusCode, Filter, Rejection, Reply};

const MAX_DATA_CENTER_ID: u8 = (1 << 5) - 1;
const MAX_WORKER_ID: u8 = (1 << 5) - 1;

const DEFAULT_EPOCH: SystemTime = UNIX_EPOCH;

#[derive(serde::Deserialize)]
struct GenerateRequest {
    count: Option<i64>,
}

#[derive(Debug, clap::Parser)]
struct Args {
    #[arg(long, default_value = "8080", env = "PORT")]
    port: u16,

    // TO SET WORKER ID AUTOMATICALLY IN A K8S STATEFUL SET, SET TO "FROM_HOSTNAME"
    #[arg(long, default_value = "0", env = "WORKER_ID")]
    worker_id: String,

    #[arg(long, default_value = "0", env = "DATA_CENTER_ID")]
    data_center_id: u8,

    #[arg(long, env = "EPOCH")]
    epoch: Option<u64>,
}

pub async fn run_worker() {
    let args = Args::parse();
    warp::serve(create_routes())
        .run(([0, 0, 0, 0], args.port))
        .await;
}

/// Returns a future which will resolve when Ctrl-C is received.
///
/// Useful to know when the process should begin its cleanup and graceful shutdown.
#[cfg(windows)]
pub async fn exit_signal() {
    let _ = tokio::signal::ctrl_c().await.unwrap();
}

/// Returns a future which will resolve when SIGINT/SIGTERM are sent to the process.
///
/// Useful to know when the process should begin its cleanup and graceful shutdown.
#[cfg(unix)]
pub async fn exit_signal() {
    use tokio::signal::unix::{Signal, SignalKind};

    let mut term = create_signal(SignalKind::terminate());
    let mut ctrl_c = create_signal(SignalKind::interrupt());

    tokio::select! {
        _ = term.recv() => {},
        _ = ctrl_c.recv() => {},
    }

    fn create_signal(kind: SignalKind) -> Signal {
        tokio::signal::unix::signal(kind).expect("couldn't create signal.")
    }
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
        .and_then(move |body: warp::hyper::body::Bytes| {
            let snowflake_generator = snowflake_generator.clone();
            async move {
                // NOTE(ayubun): We parse JSON manually to handle malformed JSON as a 400 Bad Request.
                // This decision was made because the default behavior is to silently fallback to the
                // empty body route, which generates 1 ID. I feel like this isn't as ergonomic as the
                // API telling you that you've made an error loudly so that you can fix it.
                let request: GenerateRequest = if body.is_empty() {
                    GenerateRequest { count: None }
                } else {
                    match serde_json::from_slice(&body) {
                        Ok(req) => req,
                        Err(_) => {
                            let reply = warp::reply::json(&serde_json::json!({"error": "Invalid JSON format"}));
                            return Ok(Box::new(warp::reply::with_status(reply, StatusCode::BAD_REQUEST)) as Box<dyn Reply>);
                        }
                    }
                };

                let count = request.count.unwrap_or(1);

                // NOTE(ayubun): We want to also return a 400 Bad Request for zero or negative count
                // for similar reasons to the JSON parsing.
                if count <= 0 {
                    let reply = warp::reply::json(&serde_json::json!({"error": "Invalid count: must be a positive integer"}));
                    return Ok(Box::new(warp::reply::with_status(reply, StatusCode::BAD_REQUEST)) as Box<dyn Reply>);
                }

                let mut ids: Vec<i64> = Vec::with_capacity(count as usize);
                let mut unlocked_generator = snowflake_generator.lock().await;
                for _ in 0..count {
                    ids.push(unlocked_generator.real_time_generate());
                }

                let reply = warp::reply::json(&ids);
                Ok(Box::new(warp::reply::with_status(reply, StatusCode::OK)) as Box<dyn Reply>)
            }
        });

    // TODO(ayubun): Add support for GRPC ? :3
    generate_api.or(health_api)
}

fn snowflake_id_generator_from_env() -> SnowflakeIdGenerator {
    let args = if cfg!(test) {
        // NOTE(ayubun): during tests, we should only parse from environment variables.
        // CLI args will conflict with the necessary `--test-threads=1` flag, which
        // is needed to run tests in series so that the environment variables don't conflict
        Args::try_parse_from([""]).unwrap_or_else(|_| Args::parse())
    } else {
        Args::parse()
    };

    // NOTE(ayubun): for testing, i'm allowing hostname to be set via an environment variable.
    // this is so we can ensure the hostname parsing works as expected~
    let hostname = env::var("HOSTNAME_FOR_TESTING").unwrap_or_else(|_| {
        hostname::get()
            .map(|os| os.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "localhost".to_string())
    });

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
        hostname
            .rsplit_once('-')
            .expect(
                "cannot split WORKER_ID from hostname (WORKER_ID is being parsed from hostname)",
            )
            .1
            .parse()
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
    use std::env;

    use warp::test::request;

    #[test]
    fn test_env_parsing_default_values() {
        env::remove_var("WORKER_ID");
        env::remove_var("DATA_CENTER_ID");
        env::remove_var("EPOCH");

        let mut generator = snowflake_id_generator_from_env();
        let id = generator.real_time_generate();
        assert!(id > 0);
    }

    #[test]
    fn test_env_parsing_worker_id() {
        env::set_var("WORKER_ID", "15");
        env::set_var("DATA_CENTER_ID", "0");
        env::remove_var("EPOCH");

        let mut generator = snowflake_id_generator_from_env();
        let id = generator.real_time_generate();
        assert!(id > 0);

        env::remove_var("WORKER_ID");
        env::remove_var("DATA_CENTER_ID");
    }

    #[test]
    fn test_env_parsing_data_center_id() {
        env::set_var("WORKER_ID", "0");
        env::set_var("DATA_CENTER_ID", "10");
        env::remove_var("EPOCH");

        let mut generator = snowflake_id_generator_from_env();
        let id = generator.real_time_generate();
        assert!(id > 0);

        env::remove_var("WORKER_ID");
        env::remove_var("DATA_CENTER_ID");
    }

    #[test]
    fn test_env_parsing_epoch() {
        env::set_var("WORKER_ID", "5");
        env::set_var("DATA_CENTER_ID", "3");
        env::set_var("EPOCH", "1420070400000"); // Discord's Epoch (2015-01-01 00:00:00 UTC)

        let mut generator = snowflake_id_generator_from_env();
        let id = generator.real_time_generate();
        assert!(id > 0);

        env::remove_var("WORKER_ID");
        env::remove_var("DATA_CENTER_ID");
        env::remove_var("EPOCH");
    }

    #[test]
    fn test_env_parsing_max_values() {
        env::set_var("WORKER_ID", MAX_WORKER_ID.to_string());
        env::set_var("DATA_CENTER_ID", MAX_DATA_CENTER_ID.to_string());
        env::remove_var("EPOCH");

        let mut generator = snowflake_id_generator_from_env();
        let id = generator.real_time_generate();
        assert!(id > 0);

        env::remove_var("WORKER_ID");
        env::remove_var("DATA_CENTER_ID");
    }

    #[test]
    fn test_env_parsing_hostnames() {
        let valid_hostnames = vec![
            "app-15",
            "service-worker-10",
            "my-pod-name-7",
            "test-0",
            "meow-meow-31",
        ];

        for hostname in valid_hostnames {
            env::set_var("WORKER_ID", "FROM_HOSTNAME");
            env::set_var("DATA_CENTER_ID", "0");
            env::set_var("HOSTNAME_FOR_TESTING", hostname);
            env::remove_var("EPOCH");

            let mut generator = snowflake_id_generator_from_env();
            let id = generator.real_time_generate();
            assert!(id > 0, "Failed for hostname: {hostname}");

            env::remove_var("WORKER_ID");
            env::remove_var("DATA_CENTER_ID");
            env::remove_var("HOSTNAME_FOR_TESTING");
        }
    }

    #[test]
    #[should_panic(expected = "cannot split WORKER_ID from hostname")]
    fn test_env_parsing_hostname_no_dash() {
        env::set_var("WORKER_ID", "FROM_HOSTNAME");
        env::set_var("DATA_CENTER_ID", "0");
        env::set_var("HOSTNAME_FOR_TESTING", "nodasheshere");
        env::remove_var("EPOCH");

        snowflake_id_generator_from_env();
    }

    #[test]
    #[should_panic(expected = "cannot parse WORKER_ID from hostname")]
    fn test_env_parsing_hostname_invalid_suffix() {
        env::set_var("WORKER_ID", "FROM_HOSTNAME");
        env::set_var("DATA_CENTER_ID", "0");
        env::set_var("HOSTNAME_FOR_TESTING", "hostname-invalid");
        env::remove_var("EPOCH");

        snowflake_id_generator_from_env();
    }

    #[test]
    #[should_panic(expected = "cannot parse WORKER_ID from hostname")]
    fn test_env_parsing_hostname_empty_suffix() {
        env::set_var("WORKER_ID", "FROM_HOSTNAME");
        env::set_var("DATA_CENTER_ID", "0");
        env::set_var("HOSTNAME_FOR_TESTING", "hostname-");
        env::remove_var("EPOCH");

        snowflake_id_generator_from_env();
    }

    #[test]
    #[should_panic(expected = "cannot parse WORKER_ID as a valid u8")]
    fn test_env_parsing_invalid_worker_id() {
        env::set_var("WORKER_ID", "invalid");
        env::set_var("DATA_CENTER_ID", "0");
        env::remove_var("EPOCH");

        snowflake_id_generator_from_env();
    }

    #[test]
    #[should_panic(expected = "DATA_CENTER_ID must be less than")]
    fn test_env_parsing_data_center_id_too_large() {
        env::set_var("WORKER_ID", "0");
        env::set_var("DATA_CENTER_ID", "32");
        env::remove_var("EPOCH");
        snowflake_id_generator_from_env();
    }

    #[test]
    #[should_panic(expected = "WORKER_ID must be less than")]
    fn test_env_parsing_worker_id_too_large() {
        env::set_var("WORKER_ID", "32");
        env::set_var("DATA_CENTER_ID", "0");
        env::remove_var("EPOCH");
        snowflake_id_generator_from_env();
    }

    #[test]
    #[should_panic(expected = "WORKER_ID must be less than")]
    fn test_env_parsing_hostname_worker_id_too_large() {
        env::set_var("WORKER_ID", "FROM_HOSTNAME");
        env::set_var("DATA_CENTER_ID", "0");
        env::set_var("HOSTNAME_FOR_TESTING", "hostname-32");
        env::remove_var("EPOCH");

        snowflake_id_generator_from_env();
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        env::remove_var("WORKER_ID");
        env::remove_var("DATA_CENTER_ID");
        env::remove_var("EPOCH");
        env::remove_var("HOSTNAME_FOR_TESTING");

        let routes = create_routes();

        let resp = request().method("GET").path("/health").reply(&routes).await;

        assert_eq!(resp.status(), 200);
        assert_eq!(resp.body(), "OK");
    }

    #[tokio::test]
    async fn test_generate_endpoint_no_payload() {
        env::remove_var("WORKER_ID");
        env::remove_var("DATA_CENTER_ID");
        env::remove_var("EPOCH");
        env::remove_var("HOSTNAME_FOR_TESTING");

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
        env::remove_var("WORKER_ID");
        env::remove_var("DATA_CENTER_ID");
        env::remove_var("EPOCH");
        env::remove_var("HOSTNAME_FOR_TESTING");

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
        env::remove_var("WORKER_ID");
        env::remove_var("DATA_CENTER_ID");
        env::remove_var("EPOCH");
        env::remove_var("HOSTNAME_FOR_TESTING");

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
        env::remove_var("WORKER_ID");
        env::remove_var("DATA_CENTER_ID");
        env::remove_var("EPOCH");
        env::remove_var("HOSTNAME_FOR_TESTING");

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
        env::remove_var("WORKER_ID");
        env::remove_var("DATA_CENTER_ID");
        env::remove_var("EPOCH");
        env::remove_var("HOSTNAME_FOR_TESTING");

        let routes = create_routes();

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
        env::remove_var("WORKER_ID");
        env::remove_var("DATA_CENTER_ID");
        env::remove_var("EPOCH");
        env::remove_var("HOSTNAME_FOR_TESTING");

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
        env::remove_var("WORKER_ID");
        env::remove_var("DATA_CENTER_ID");
        env::remove_var("EPOCH");
        env::remove_var("HOSTNAME_FOR_TESTING");

        let routes = create_routes();

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
