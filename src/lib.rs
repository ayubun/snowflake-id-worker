//! http routing, process wiring, and backpressure

mod generator;

use clap::Parser;
use generator::{Clock, SnowflakeGenerator, MAX_TIMESTAMP_MILLIS};
use std::{
    convert::Infallible,
    env,
    panic::{self, AssertUnwindSafe},
    process,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::{mpsc, oneshot};
use warp::http::header::RETRY_AFTER;
use warp::http::StatusCode;
use warp::hyper::body::Bytes;
use warp::reply::Response;
use warp::{Filter, Reply};

const MAX_DATA_CENTER_ID: u8 = (1 << 5) - 1;
const MAX_WORKER_ID: u8 = (1 << 5) - 1;

const DEFAULT_EPOCH_MILLIS: i64 = 0;

/// default per-request generation cap
const DEFAULT_MAX_BATCH_SIZE: usize = 100_000;

/// hard cap keeps one request below about 80 mb
const SAFE_MAX_BATCH_SIZE: usize = 10_000_000;

/// maximum accepted request body size
const MAX_BODY_BYTES: u64 = 1_024;

/// bounded queue depth before requests receive 429
const GENERATION_QUEUE_CAPACITY: usize = 256;

/// shared by the json body on `POST` and the query string on `GET`
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct GenerateRequest {
    count: Option<i64>,
}

#[derive(Debug, clap::Parser)]
struct Args {
    #[arg(long, default_value = "8080", env = "PORT")]
    port: u16,

    // use from_hostname for k8s stateful sets
    #[arg(long, default_value = "0", env = "WORKER_ID")]
    worker_id: String,

    #[arg(long, default_value = "0", env = "DATA_CENTER_ID")]
    data_center_id: u8,

    #[arg(long, env = "EPOCH")]
    epoch: Option<u64>,

    #[arg(
        long,
        default_value_t = DEFAULT_MAX_BATCH_SIZE as u64,
        env = "MAX_BATCH_SIZE",
        value_parser = clap::value_parser!(u64).range(1..=SAFE_MAX_BATCH_SIZE as u64)
    )]
    max_batch_size: u64,
}

pub async fn run_worker() {
    let args = Args::parse();
    let port = args.port;
    // graceful shutdown lets in-flight requests finish
    let (_addr, server) = warp::serve(create_routes_from_args(args)).bind_with_graceful_shutdown(
        ([0, 0, 0, 0], port),
        async {
            exit_signal().await;
            println!("exiting from signal");
        },
    );
    server.await;
    println!("worker exited");
}

/// waits for ctrl-c
#[cfg(windows)]
pub async fn exit_signal() {
    tokio::signal::ctrl_c().await;
}

/// waits for sigint or sigterm
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

/// builds the routes from environment variables; used by tests and benches
pub fn create_routes() -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    create_routes_from_args(parse_env_args())
}

fn create_routes_from_args(
    args: Args,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let max_batch_size = args.max_batch_size as usize;
    // one thread owns generation while the channel provides backpressure
    let (job_sender, job_receiver) = mpsc::channel::<GenerateJob>(GENERATION_QUEUE_CAPACITY);
    spawn_generator_thread(snowflake_generator(args), job_receiver);

    let health_api = warp::path!("health").and(warp::get()).map(|| "OK");

    let generate_path = warp::path!("generate");

    let post_sender = job_sender.clone();
    let generate_post = generate_path
        .and(warp::post())
        // reject oversized bodies before buffering them
        .and(warp::body::content_length_limit(MAX_BODY_BYTES))
        .and(warp::body::bytes())
        .then(move |body: Bytes| {
            let job_sender = post_sender.clone();
            async move { handle_generate_post(body, job_sender, max_batch_size).await }
        });

    let generate_get = generate_path
        .and(warp::get())
        .and(raw_query_or_empty())
        .then(move |query: String| {
            let job_sender = job_sender.clone();
            async move { handle_generate_get(query, job_sender, max_batch_size).await }
        });

    // todo(ayubun): add grpc support
    generate_post.or(generate_get).or(health_api)
}

/// `warp::query::raw` rejects requests without a query string; treat those as empty
fn raw_query_or_empty() -> impl Filter<Extract = (String,), Error = Infallible> + Clone {
    warp::query::raw().or(warp::any().map(String::new)).unify()
}

struct GenerateJob {
    count: usize,
    reply: oneshot::Sender<Vec<i64>>,
}

/// owns generation on one thread to avoid shared counter contention
fn spawn_generator_thread(
    generator: SnowflakeGenerator<SystemClock>,
    mut job_receiver: mpsc::Receiver<GenerateJob>,
) {
    std::thread::spawn(move || {
        while let Some(job) = job_receiver.blocking_recv() {
            // skip work when the requester already disconnected
            if job.reply.is_closed() {
                continue;
            }
            let outcome =
                panic::catch_unwind(AssertUnwindSafe(|| generator.generate_batch(job.count)));
            match outcome {
                Ok(ids) => {
                    let _ = job.reply.send(ids);
                }
                Err(_) => {
                    eprintln!("snowflake generator thread panicked; aborting process");
                    process::abort();
                }
            }
        }
    });
}

async fn handle_generate_post(
    body: Bytes,
    job_sender: mpsc::Sender<GenerateJob>,
    max_batch_size: usize,
) -> Response {
    // parse manually so malformed json cannot fall back to one id
    let request: Option<GenerateRequest> = if body.is_empty() {
        None
    } else {
        match serde_json::from_slice(&body) {
            Ok(req) => Some(req),
            Err(_) => return text_response(StatusCode::BAD_REQUEST, "invalid json format".into()),
        }
    };

    generate_response(request.and_then(|r| r.count), job_sender, max_batch_size).await
}

async fn handle_generate_get(
    query: String,
    job_sender: mpsc::Sender<GenerateJob>,
    max_batch_size: usize,
) -> Response {
    // an empty query string deserializes to no count, which means one id
    let request: GenerateRequest = match serde_urlencoded::from_str(&query) {
        Ok(req) => req,
        Err(_) => return text_response(StatusCode::BAD_REQUEST, "invalid query string".into()),
    };

    generate_response(request.count, job_sender, max_batch_size).await
}

/// validates the requested count and hands the job to the generator thread
async fn generate_response(
    requested_count: Option<i64>,
    job_sender: mpsc::Sender<GenerateJob>,
    max_batch_size: usize,
) -> Response {
    let count = requested_count.unwrap_or(1);

    if count <= 0 {
        return text_response(
            StatusCode::BAD_REQUEST,
            "invalid count: must be a positive integer".into(),
        );
    }

    // compare after positivity validation so the cast cannot wrap
    if count as usize > max_batch_size {
        return text_response(
            StatusCode::BAD_REQUEST,
            format!("invalid count: must not exceed {max_batch_size}"),
        );
    }

    let count = count as usize;
    let (reply_sender, reply_receiver) = oneshot::channel();

    // never block the async worker when the bounded queue is full
    match job_sender.try_send(GenerateJob {
        count,
        reply: reply_sender,
    }) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(_)) => return saturated_response(),
        Err(mpsc::error::TrySendError::Closed(_)) => {
            return text_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "generator unavailable".into(),
            );
        }
    }

    match reply_receiver.await {
        Ok(ids) => json_response(&ids),
        Err(_) => text_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to generate ids".into(),
        ),
    }
}

fn json_response(ids: &[i64]) -> Response {
    warp::reply::json(&ids).into_response()
}

fn text_response(status: StatusCode, message: String) -> Response {
    warp::reply::with_status(message, status).into_response()
}

fn saturated_response() -> Response {
    warp::reply::with_header(
        warp::reply::with_status(
            "saturated: generation queue is full, retry shortly".to_string(),
            StatusCode::TOO_MANY_REQUESTS,
        ),
        RETRY_AFTER,
        "1",
    )
    .into_response()
}

#[cfg(test)]
fn snowflake_generator_from_env() -> SnowflakeGenerator<SystemClock> {
    snowflake_generator(parse_env_args())
}

/// reads worker settings from the environment only, because test and bench
/// harnesses own the process argv
fn parse_env_args() -> Args {
    Args::try_parse_from([""]).expect("environment variables hold valid worker settings")
}

fn snowflake_generator(args: Args) -> SnowflakeGenerator<SystemClock> {
    // test hostname parsing without changing the machine hostname
    let hostname = env::var("HOSTNAME_FOR_TESTING").unwrap_or_else(|_| {
        hostname::get()
            .map(|os| os.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "localhost".to_string())
    });

    // reject epochs that would wrap into a negative value
    let epoch_millis: i64 = match args.epoch {
        Some(e) => i64::try_from(e).unwrap_or_else(|_| panic!("EPOCH is too large (EPOCH: {e})")),
        None => DEFAULT_EPOCH_MILLIS,
    };

    let worker_id = if args.worker_id.eq_ignore_ascii_case("FROM_HOSTNAME") {
        // stateful set hostnames end in the pod index
        hostname
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
        panic!("DATA_CENTER_ID must be at most {MAX_DATA_CENTER_ID}");
    }

    if worker_id > MAX_WORKER_ID {
        panic!("WORKER_ID must be at most {MAX_WORKER_ID}");
    }

    let clock = SystemClock;
    let now_millis = clock.now_unix_millis();
    // a future epoch would produce negative timestamps
    if epoch_millis > now_millis {
        panic!("EPOCH must not be in the future (EPOCH: {epoch_millis}, now: {now_millis})");
    }
    // reserve one millisecond because the first id waits past startup
    let relative_now = now_millis - epoch_millis;
    if relative_now >= MAX_TIMESTAMP_MILLIS {
        panic!(
            "EPOCH is too far in the past: relative time {relative_now}ms reaches the {MAX_TIMESTAMP_MILLIS}ms (41-bit) timestamp range"
        );
    }

    println!("starting snowflake-id-worker with WORKER_ID: {worker_id}, DATA_CENTER_ID: {}, and EPOCH: {epoch_millis}", args.data_center_id);

    SnowflakeGenerator::new(args.data_center_id, worker_id, epoch_millis, clock)
}

struct SystemClock;

impl Clock for SystemClock {
    fn now_unix_millis(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is before the unix epoch")
            .as_millis() as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use serial_test::serial;
    use std::collections::HashSet;
    use std::env;

    use warp::test::request;

    #[test]
    #[serial]
    fn test_env_parsing_default_values() {
        env::remove_var("WORKER_ID");
        env::remove_var("DATA_CENTER_ID");
        env::remove_var("EPOCH");

        let generator = snowflake_generator_from_env();
        let id = generator.generate();
        assert!(id > 0);
    }

    #[test]
    #[serial]
    fn test_env_parsing_worker_id() {
        env::set_var("WORKER_ID", "15");
        env::set_var("DATA_CENTER_ID", "0");
        env::remove_var("EPOCH");

        let generator = snowflake_generator_from_env();
        let id = generator.generate();
        assert!(id > 0);

        env::remove_var("WORKER_ID");
        env::remove_var("DATA_CENTER_ID");
    }

    #[test]
    #[serial]
    fn test_env_parsing_data_center_id() {
        env::set_var("WORKER_ID", "0");
        env::set_var("DATA_CENTER_ID", "10");
        env::remove_var("EPOCH");

        let generator = snowflake_generator_from_env();
        let id = generator.generate();
        assert!(id > 0);

        env::remove_var("WORKER_ID");
        env::remove_var("DATA_CENTER_ID");
    }

    #[test]
    #[serial]
    fn test_env_parsing_epoch() {
        env::set_var("WORKER_ID", "5");
        env::set_var("DATA_CENTER_ID", "3");
        env::set_var("EPOCH", "1420070400000"); // Discord's Epoch (2015-01-01 00:00:00 UTC)

        let generator = snowflake_generator_from_env();
        let id = generator.generate();
        assert!(id > 0);

        env::remove_var("WORKER_ID");
        env::remove_var("DATA_CENTER_ID");
        env::remove_var("EPOCH");
    }

    #[test]
    #[serial]
    fn test_env_parsing_max_values() {
        env::set_var("WORKER_ID", MAX_WORKER_ID.to_string());
        env::set_var("DATA_CENTER_ID", MAX_DATA_CENTER_ID.to_string());
        env::remove_var("EPOCH");

        let generator = snowflake_generator_from_env();
        let id = generator.generate();
        assert!(id > 0);

        env::remove_var("WORKER_ID");
        env::remove_var("DATA_CENTER_ID");
    }

    #[test]
    #[serial]
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

            let generator = snowflake_generator_from_env();
            let id = generator.generate();
            assert!(id > 0, "Failed for hostname: {hostname}");

            env::remove_var("WORKER_ID");
            env::remove_var("DATA_CENTER_ID");
            env::remove_var("HOSTNAME_FOR_TESTING");
        }
    }

    #[test]
    #[serial]
    #[should_panic(expected = "cannot split WORKER_ID from hostname")]
    fn test_env_parsing_hostname_no_dash() {
        env::set_var("WORKER_ID", "FROM_HOSTNAME");
        env::set_var("DATA_CENTER_ID", "0");
        env::set_var("HOSTNAME_FOR_TESTING", "nodasheshere");
        env::remove_var("EPOCH");

        snowflake_generator_from_env();
    }

    #[test]
    #[serial]
    #[should_panic(expected = "cannot parse WORKER_ID from hostname")]
    fn test_env_parsing_hostname_invalid_suffix() {
        env::set_var("WORKER_ID", "FROM_HOSTNAME");
        env::set_var("DATA_CENTER_ID", "0");
        env::set_var("HOSTNAME_FOR_TESTING", "hostname-invalid");
        env::remove_var("EPOCH");

        snowflake_generator_from_env();
    }

    #[test]
    #[serial]
    #[should_panic(expected = "cannot parse WORKER_ID from hostname")]
    fn test_env_parsing_hostname_empty_suffix() {
        env::set_var("WORKER_ID", "FROM_HOSTNAME");
        env::set_var("DATA_CENTER_ID", "0");
        env::set_var("HOSTNAME_FOR_TESTING", "hostname-");
        env::remove_var("EPOCH");

        snowflake_generator_from_env();
    }

    #[test]
    #[serial]
    #[should_panic(expected = "cannot parse WORKER_ID as a valid u8")]
    fn test_env_parsing_invalid_worker_id() {
        env::set_var("WORKER_ID", "invalid");
        env::set_var("DATA_CENTER_ID", "0");
        env::remove_var("EPOCH");

        snowflake_generator_from_env();
    }

    #[test]
    #[serial]
    #[should_panic(expected = "DATA_CENTER_ID must be at most")]
    fn test_env_parsing_data_center_id_too_large() {
        env::set_var("WORKER_ID", "0");
        env::set_var("DATA_CENTER_ID", "32");
        env::remove_var("EPOCH");

        snowflake_generator_from_env();
    }

    #[test]
    #[serial]
    #[should_panic(expected = "WORKER_ID must be at most")]
    fn test_env_parsing_worker_id_too_large() {
        env::set_var("WORKER_ID", "32");
        env::set_var("DATA_CENTER_ID", "0");
        env::remove_var("EPOCH");

        snowflake_generator_from_env();
    }

    #[test]
    #[serial]
    #[should_panic(expected = "WORKER_ID must be at most")]
    fn test_env_parsing_hostname_worker_id_too_large() {
        env::set_var("WORKER_ID", "FROM_HOSTNAME");
        env::set_var("DATA_CENTER_ID", "0");
        env::set_var("HOSTNAME_FOR_TESTING", "hostname-32");
        env::remove_var("EPOCH");

        snowflake_generator_from_env();
    }

    #[tokio::test]
    #[serial]
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
    #[serial]
    async fn test_generate_endpoint_no_payload() {
        env::remove_var("WORKER_ID");
        env::remove_var("DATA_CENTER_ID");
        env::remove_var("EPOCH");
        env::remove_var("HOSTNAME_FOR_TESTING");

        let routes = create_routes();

        // an empty body still yields one id
        let resp = request()
            .method("POST")
            .path("/generate")
            .body("")
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
    #[serial]
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
    #[serial]
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
    #[serial]
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
    #[serial]
    async fn test_invalid_request_methods() {
        env::remove_var("WORKER_ID");
        env::remove_var("DATA_CENTER_ID");
        env::remove_var("EPOCH");
        env::remove_var("HOSTNAME_FOR_TESTING");

        let routes = create_routes();

        // generate accepts get and post only
        let resp = request()
            .method("PUT")
            .path("/generate")
            .reply(&routes)
            .await;

        assert_eq!(resp.status(), 405); // method not allowed

        // health only accepts get
        let resp = request()
            .method("POST")
            .path("/health")
            .reply(&routes)
            .await;

        assert_eq!(resp.status(), 405); // method not allowed
    }

    #[tokio::test]
    #[serial]
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

        assert_eq!(resp.status(), 404);
    }

    #[tokio::test]
    #[serial]
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
            body.contains("invalid count"),
            "should contain error message about invalid count"
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
            body.contains("invalid count"),
            "should contain error message about invalid count"
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
            body.contains("invalid json"),
            "should contain error message about invalid json"
        );
    }

    fn clear_env() {
        env::remove_var("WORKER_ID");
        env::remove_var("DATA_CENTER_ID");
        env::remove_var("EPOCH");
        env::remove_var("HOSTNAME_FOR_TESTING");
        env::remove_var("MAX_BATCH_SIZE");
    }

    #[tokio::test]
    #[serial]
    async fn test_generate_response_is_json_content_type() {
        clear_env();
        let routes = create_routes();

        let resp = request()
            .method("POST")
            .path("/generate")
            .json(&json!({"count": 3}))
            .reply(&routes)
            .await;

        assert_eq!(resp.status(), 200);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/json",
            "generate responses must advertise json"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_generate_rejects_count_over_max_batch_size() {
        clear_env();
        let routes = create_routes();

        let resp = request()
            .method("POST")
            .path("/generate")
            .json(&json!({"count": DEFAULT_MAX_BATCH_SIZE as i64 + 1}))
            .reply(&routes)
            .await;

        assert_eq!(resp.status(), 400);
        let body = std::str::from_utf8(resp.body()).unwrap();
        assert!(
            body.contains("must not exceed"),
            "should explain the batch cap, got: {body}"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_generate_accepts_count_at_max_batch_size() {
        clear_env();
        let routes = create_routes();

        let resp = request()
            .method("POST")
            .path("/generate")
            .json(&json!({"count": DEFAULT_MAX_BATCH_SIZE as i64}))
            .reply(&routes)
            .await;

        assert_eq!(resp.status(), 200, "the cap itself must be allowed");
        let ids: Vec<i64> =
            serde_json::from_str(std::str::from_utf8(resp.body()).unwrap()).unwrap();
        assert_eq!(ids.len(), DEFAULT_MAX_BATCH_SIZE);
    }

    #[tokio::test]
    #[serial]
    async fn test_generate_rejects_unknown_fields() {
        clear_env();
        let routes = create_routes();

        // reject typos instead of silently generating one id
        let resp = request()
            .method("POST")
            .path("/generate")
            .json(&json!({"cont": 10}))
            .reply(&routes)
            .await;

        assert_eq!(resp.status(), 400);
    }

    #[tokio::test]
    #[serial]
    async fn test_generate_rejects_oversized_body() {
        clear_env();
        let routes = create_routes();

        let oversized = "a".repeat(MAX_BODY_BYTES as usize + 1);
        let resp = request()
            .method("POST")
            .path("/generate")
            .body(oversized)
            .reply(&routes)
            .await;

        assert_eq!(resp.status(), 413);
    }

    #[test]
    #[serial]
    fn test_saturated_response_carries_retry_after() {
        let resp = saturated_response();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            resp.headers().get(RETRY_AFTER).unwrap(),
            "1",
            "429 responses must tell clients when to retry"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_get_generate_without_query_returns_one_id() {
        clear_env();
        let routes = create_routes();

        let resp = request()
            .method("GET")
            .path("/generate")
            .reply(&routes)
            .await;

        assert_eq!(resp.status(), 200);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/json"
        );
        let ids: Vec<i64> = serde_json::from_slice(resp.body()).unwrap();
        assert_eq!(ids.len(), 1);
        assert!(ids[0] > 0);
    }

    #[tokio::test]
    #[serial]
    async fn test_get_generate_with_count_query() {
        clear_env();
        let routes = create_routes();

        let resp = request()
            .method("GET")
            .path("/generate?count=10")
            .reply(&routes)
            .await;

        assert_eq!(resp.status(), 200);
        let ids: Vec<i64> = serde_json::from_slice(resp.body()).unwrap();
        assert_eq!(ids.len(), 10);
        let unique: HashSet<i64> = ids.iter().cloned().collect();
        assert_eq!(unique.len(), 10, "all ids should be unique");
    }

    #[tokio::test]
    #[serial]
    async fn test_get_generate_rejects_non_positive_count() {
        clear_env();
        let routes = create_routes();

        for query in ["count=0", "count=-5"] {
            let resp = request()
                .method("GET")
                .path(&format!("/generate?{query}"))
                .reply(&routes)
                .await;
            assert_eq!(resp.status(), 400, "{query} should be rejected");
            let body = std::str::from_utf8(resp.body()).unwrap();
            assert!(body.contains("invalid count"), "unexpected body: {body}");
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_get_generate_rejects_malformed_query() {
        clear_env();
        let routes = create_routes();

        // a non-numeric count and an unknown field both fail query parsing
        for query in ["count=abc", "count=", "foo=1", "count=1&foo=1"] {
            let resp = request()
                .method("GET")
                .path(&format!("/generate?{query}"))
                .reply(&routes)
                .await;
            assert_eq!(resp.status(), 400, "{query} should be rejected");
            let body = std::str::from_utf8(resp.body()).unwrap();
            assert!(body.contains("invalid query"), "unexpected body: {body}");
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_get_generate_rejects_count_above_max() {
        clear_env();
        let routes = create_routes();

        let resp = request()
            .method("GET")
            .path(&format!("/generate?count={}", DEFAULT_MAX_BATCH_SIZE + 1))
            .reply(&routes)
            .await;

        assert_eq!(resp.status(), 400);
        let body = std::str::from_utf8(resp.body()).unwrap();
        assert!(
            body.contains(&format!("must not exceed {DEFAULT_MAX_BATCH_SIZE}")),
            "unexpected body: {body}"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_get_generate_ignores_request_body() {
        clear_env();
        let routes = create_routes();

        // the query decides the count on GET; a json body is not consulted
        let resp = request()
            .method("GET")
            .path("/generate?count=2")
            .json(&json!({"count": 50}))
            .reply(&routes)
            .await;

        assert_eq!(resp.status(), 200);
        let ids: Vec<i64> = serde_json::from_slice(resp.body()).unwrap();
        assert_eq!(ids.len(), 2);
    }
}
