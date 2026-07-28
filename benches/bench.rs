use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use serde_json::json;
use snowflake_id_worker::create_routes;
use warp::test::request;

fn bench_single_generate(c: &mut Criterion) {
    c.bench_function("single_generate", |b| {
        b.iter(|| {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let routes = create_routes();
                let resp = request()
                    .method("POST")
                    .path("/generate")
                    // send content-length for the empty body
                    .body("")
                    .reply(&routes)
                    .await;
                black_box(resp)
            })
        })
    });
}

fn bench_batch_generate(c: &mut Criterion) {
    // raise the cap for large batch benchmarks
    std::env::set_var("MAX_BATCH_SIZE", "10000000");

    let mut group = c.benchmark_group("batch_generate");

    for size in [10, 100, 1_000, 10_000, 100_000, 1_000_000, 10_000_000].iter() {
        group.bench_with_input(BenchmarkId::new("batch", size), size, |b, &size| {
            let payload = json!({"count": size});
            b.iter(|| {
                tokio::runtime::Runtime::new().unwrap().block_on(async {
                    let routes = create_routes();
                    let resp = request()
                        .method("POST")
                        .path("/generate")
                        .json(&payload)
                        .reply(&routes)
                        .await;
                    black_box(resp)
                })
            })
        });
    }
    group.finish();
}

fn bench_concurrent_single_generates(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_single_generates");
    for concurrent_requests in [
        2, 10, 20, 50, 100, 200, 500, 1_000, 10_000, 100_000, 1_000_000,
    ]
    .iter()
    {
        group.bench_with_input(
            BenchmarkId::new("Num Concurrent Requests", concurrent_requests),
            concurrent_requests,
            |b, &concurrent_requests| {
                b.iter(|| {
                    tokio::runtime::Runtime::new().unwrap().block_on(async {
                        let routes = create_routes();
                        let payload = json!({"count": 1});

                        let mut handles = Vec::new();
                        for _ in 0..concurrent_requests {
                            let routes_clone = routes.clone();
                            let payload_clone = payload.clone();

                            let handle = tokio::spawn(async move {
                                request()
                                    .method("POST")
                                    .path("/generate")
                                    .json(&payload_clone)
                                    .reply(&routes_clone)
                                    .await
                            });
                            handles.push(handle);
                        }

                        let mut responses = Vec::new();
                        for handle in handles {
                            responses.push(handle.await.unwrap());
                        }
                        black_box(responses)
                    })
                })
            },
        );
    }
    group.finish();
}

fn bench_concurrent_batch_generates(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_batch_generates");
    // results depend on the default tokio thread count
    for concurrent_requests in [2, 4, 6, 8, 10, 20, 50, 100, 200, 500, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::new("Num Concurrent Requests", concurrent_requests),
            concurrent_requests,
            |b, &concurrent_requests| {
                b.iter(|| {
                    tokio::runtime::Runtime::new().unwrap().block_on(async {
                        let routes = create_routes();
                        let payload = json!({"count": 100});

                        let mut handles = Vec::new();
                        for _ in 0..concurrent_requests {
                            let routes_clone = routes.clone();
                            let payload_clone = payload.clone();
                            let handle = tokio::spawn(async move {
                                request()
                                    .method("POST")
                                    .path("/generate")
                                    .json(&payload_clone)
                                    .reply(&routes_clone)
                                    .await
                            });
                            handles.push(handle);
                        }

                        let mut responses = Vec::new();
                        for handle in handles {
                            responses.push(handle.await.unwrap());
                        }
                        black_box(responses)
                    })
                })
            },
        );
    }
    group.finish();
}

fn bench_http_error_handling(c: &mut Criterion) {
    let mut group = c.benchmark_group("http_error_handling");

    group.bench_function("zero_count_error", |b| {
        let payload = json!({"count": 0});
        b.iter(|| {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let routes = create_routes();
                let resp = request()
                    .method("POST")
                    .path("/generate")
                    .json(&payload)
                    .reply(&routes)
                    .await;
                black_box(resp)
            })
        })
    });

    group.bench_function("negative_count_error", |b| {
        let payload = json!({"count": -5});
        b.iter(|| {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let routes = create_routes();
                let resp = request()
                    .method("POST")
                    .path("/generate")
                    .json(&payload)
                    .reply(&routes)
                    .await;
                black_box(resp)
            })
        })
    });

    group.bench_function("malformed_json_error", |b| {
        b.iter(|| {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let routes = create_routes();
                let resp = request()
                    .method("POST")
                    .path("/generate")
                    .body(b"invalid json")
                    .reply(&routes)
                    .await;
                black_box(resp)
            })
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_single_generate,
    bench_batch_generate,
    bench_concurrent_single_generates,
    bench_concurrent_batch_generates,
    bench_http_error_handling
);
criterion_main!(benches);
