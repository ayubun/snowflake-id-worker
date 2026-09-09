//! in-process request benchmarks on a single-threaded tokio runtime

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use serde_json::json;
use snowflake_id_worker::create_routes;
use tokio::runtime::{Builder, Runtime};
use tokio::task::JoinSet;
use warp::http::StatusCode;
use warp::test::request;

/// one async thread keeps results comparable across machines; generation still
/// runs on the worker's dedicated generator thread
fn single_thread_runtime() -> Runtime {
    Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime")
}

/// one `POST /generate` per iteration; an empty body requests a single id
fn bench_generate(c: &mut Criterion) {
    let runtime = single_thread_runtime();
    let routes = create_routes();
    let mut group = c.benchmark_group("generate");

    for count in [1usize, 10, 100, 1_000, 10_000, 100_000] {
        let body = if count == 1 {
            String::new()
        } else {
            json!({"count": count}).to_string()
        };
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::new("count", count), &body, |b, body| {
            b.to_async(&runtime).iter(|| async {
                let resp = request()
                    .method("POST")
                    .path("/generate")
                    .body(body)
                    .reply(&routes)
                    .await;
                assert_eq!(resp.status(), StatusCode::OK);
                resp
            })
        });
    }
    group.finish();
}

/// many single-id requests in flight at once; compare against `generate/count`
/// at the same total id count to see the cost of not batching
fn bench_concurrent_single_generates(c: &mut Criterion) {
    let runtime = single_thread_runtime();
    let routes = create_routes();
    let mut group = c.benchmark_group("concurrent_single_generates");

    // stay under the 256-deep generation queue so no request receives 429
    for requests in [10usize, 100] {
        group.throughput(Throughput::Elements(requests as u64));
        group.bench_with_input(
            BenchmarkId::new("requests", requests),
            &requests,
            |b, &requests| {
                b.to_async(&runtime).iter(|| async {
                    let mut in_flight = JoinSet::new();
                    for _ in 0..requests {
                        let routes = routes.clone();
                        in_flight.spawn(async move {
                            request()
                                .method("POST")
                                .path("/generate")
                                .body("")
                                .reply(&routes)
                                .await
                        });
                    }
                    let mut responses = Vec::with_capacity(requests);
                    while let Some(joined) = in_flight.join_next().await {
                        let resp = joined.expect("request task panicked");
                        assert_eq!(resp.status(), StatusCode::OK);
                        responses.push(resp);
                    }
                    responses
                })
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_generate, bench_concurrent_single_generates);
criterion_main!(benches);
