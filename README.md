# Summary
`snowflake-id-worker` is a [Docker image](https://docs.docker.com/get-started/docker-concepts/the-basics/what-is-an-image/) published to
[GitHub's Container Registry](https://github.blog/news-insights/product-news/introducing-github-container-registry/) that serves HTTP APIs. It allows callers to generate unique [snowflake IDs](https://en.wikipedia.org/wiki/Snowflake_ID) across a
distributed system using Twitter's snowflake algorithm.

Callers can choose to generate a singular snowflake ID or supply a count in the JSON body to
generate a batch. The worker is written in Rust to optimize for performance~

Looking to learn how to host a `snowflake-id-worker` image? See [HOSTING](./HOSTING.md)

# Supported Environment Variables

The worker supports the following environment variables:

| Environment Variable | Default Value | Supported Type | Description |
|--|--|--|--|
| `WORKER_ID` | `0` | `u8` or "`FROM_HOSTNAME`" | An identifier for the given worker. Setting this value to "`FROM_HOSTNAME`" will try to parse the worker ID from the end of the hostname. This feature is for workers being run in k8s StatefulSets |
| `DATA_CENTER_ID` | `0` | `u8` | An identifier for the location that a given set of workers are running on |
| `EPOCH` | UNIX Epoch | `u64` | An optional environment variable that allows hosts to use a custom epoch. For example, Discord uses a custom epoch of `1420070400000` |

> [!IMPORTANT] 
> To ensure the uniqueness of Snowflake IDs generated across a distributed system, all workers must have a unique combination
> of `WORKER_ID` and `DATA_CENTER_ID`

# API Spec

### **POST** `/generate`
---
This endpoint can be used to generate
batches or singular snowflake IDs

**BATCH:**

If a `count` is specified in the request body (i.e. `{"count":10}`), the endpoint will return a batch of snowflake IDs with the requested count:
![`POST /generate` with populated request body](assets/generate-example-populated-body.png)

**SINGLE:**

If a `count` is **not** specified in the request body, one snowflake ID will be returned:
![`POST /generate` with empty request body](assets/generate-example-empty-body.png)

> [!NOTE]
> The API will always return a list for consistency, even when returning a single snowflake ID

### Benchmarks & Optimization Notes

---

> [!IMPORTANT] 
> The overall bench results imply that it is significantly more efficient to generate batches of snowflakes. If high throughput per
> worker is essential for your use-case, you will want to factor batching into the design of your clients.

The following benchmarks were performed on an Apple M1 Max (8 performance cores, 2 efficiency cores). Benchmark results will vary depending on the machine you perform them on. 

---

<details>
<summary><strong>Running</strong> <code>cargo bench concurrent_single_generates</code><strong>:</strong></summary>

*Raw console output:*
```
concurrent_single_generates/Num Concurrent Requests/2
                        time:   [263.34 µs 268.70 µs 275.66 µs]
                        change: [+5.8956% +8.5359% +11.966%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 7 outliers among 100 measurements (7.00%)
  2 (2.00%) low mild
  1 (1.00%) high mild
  4 (4.00%) high severe
concurrent_single_generates/Num Concurrent Requests/10
                        time:   [278.64 µs 280.12 µs 281.76 µs]
                        change: [-0.7562% +2.6252% +5.8729%] (p = 0.12 > 0.05)
                        No change in performance detected.
Found 9 outliers among 100 measurements (9.00%)
  2 (2.00%) high mild
  7 (7.00%) high severe
concurrent_single_generates/Num Concurrent Requests/20
                        time:   [315.34 µs 324.13 µs 334.49 µs]
                        change: [+3.1232% +6.0181% +8.7006%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 6 outliers among 100 measurements (6.00%)
  4 (4.00%) high mild
  2 (2.00%) high severe
concurrent_single_generates/Num Concurrent Requests/50
                        time:   [393.76 µs 411.10 µs 432.01 µs]
                        change: [-19.920% -12.821% -5.6858%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 9 outliers among 100 measurements (9.00%)
  3 (3.00%) high mild
  6 (6.00%) high severe
concurrent_single_generates/Num Concurrent Requests/100
                        time:   [488.86 µs 514.40 µs 551.89 µs]
                        change: [-1.2583% +4.0012% +10.702%] (p = 0.20 > 0.05)
                        No change in performance detected.
Found 8 outliers among 100 measurements (8.00%)
  3 (3.00%) high mild
  5 (5.00%) high severe
concurrent_single_generates/Num Concurrent Requests/200
                        time:   [701.39 µs 707.50 µs 714.44 µs]
                        change: [-18.724% -14.526% -10.781%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 1 outliers among 100 measurements (1.00%)
  1 (1.00%) high mild
Benchmarking concurrent_single_generates/Num Concurrent Requests/500: Warming up for 3.0000 s
Warning: Unable to complete 100 samples in 5.0s. You may wish to increase target time to 6.8s, enable flat sampling, or reduce sample count to 60.
concurrent_single_generates/Num Concurrent Requests/500
                        time:   [1.4297 ms 1.4559 ms 1.4921 ms]
                        change: [-1.2106% +4.1416% +10.287%] (p = 0.18 > 0.05)
                        No change in performance detected.
Found 10 outliers among 100 measurements (10.00%)
  3 (3.00%) high mild
  7 (7.00%) high severe
concurrent_single_generates/Num Concurrent Requests/1000
                        time:   [2.3843 ms 2.3949 ms 2.4065 ms]
                        change: [-8.1474% -5.4694% -3.4899%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 5 outliers among 100 measurements (5.00%)
  2 (2.00%) high mild
  3 (3.00%) high severe
concurrent_single_generates/Num Concurrent Requests/10000
                        time:   [20.438 ms 20.879 ms 21.617 ms]
Found 3 outliers among 100 measurements (3.00%)
  2 (2.00%) high mild
  1 (1.00%) high severe
Benchmarking concurrent_single_generates/Num Concurrent Requests/100000: Warming up for 3.0000 s
Warning: Unable to complete 100 samples in 5.0s. You may wish to increase target time to 21.0s, or reduce sample count to 20.
concurrent_single_generates/Num Concurrent Requests/100000
                        time:   [205.62 ms 207.60 ms 210.02 ms]
Found 8 outliers among 100 measurements (8.00%)
  1 (1.00%) low mild
  2 (2.00%) high mild
  5 (5.00%) high severe
Benchmarking concurrent_single_generates/Num Concurrent Requests/1000000: Warming up for 3.0000 s
Warning: Unable to complete 100 samples in 5.0s. You may wish to increase target time to 206.7s, or reduce sample count to 10.
concurrent_single_generates/Num Concurrent Requests/1000000
                        time:   [2.0523 s 2.0643 s 2.0765 s]
Found 1 outliers among 100 measurements (1.00%)
  1 (1.00%) high mild
```
</details>


| Concurrent Requests | Latency | Theoretical Throughput |
|-------------------------|-------------|------------------------|
| 2 | 269μs | 7,444 req/sec |
| 10 | 280μs | 35,702 req/sec |
| 20 | 324μs | 61,718 req/sec |
| 50 | 411μs | 121,634 req/sec |
| 100 | 514μs | 194,403 req/sec |
| 200 | 708μs | 282,685 req/sec |
| 500 | 1.46ms | 343,445 req/sec |
| 1,000 | 2.39ms | 417,563 req/sec |
| 10,000 | 20.9ms | 478,947 req/sec |
| 100,000 | 208ms | 481,618 req/sec |
| 1,000,000 | 2.06s | 484,438 req/sec |

---

<details>
<summary><strong>Running</strong> <code>cargo bench concurrent_batch_generates</code><strong>:</strong></summary>

*Raw console output:*
```
concurrent_batch_generates/Num Concurrent Requests/2
                        time:   [273.05 µs 279.10 µs 286.71 µs]
                        change: [+1.0647% +2.9058% +4.7268%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 7 outliers among 100 measurements (7.00%)
  4 (4.00%) high mild
  3 (3.00%) high severe
concurrent_batch_generates/Num Concurrent Requests/4
                        time:   [295.17 µs 300.76 µs 310.53 µs]
                        change: [+2.3508% +5.7233% +10.772%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 9 outliers among 100 measurements (9.00%)
  6 (6.00%) high mild
  3 (3.00%) high severe
concurrent_batch_generates/Num Concurrent Requests/6
                        time:   [327.68 µs 335.50 µs 345.46 µs]
                        change: [-0.8847% +7.0589% +16.790%] (p = 0.15 > 0.05)
                        No change in performance detected.
Found 4 outliers among 100 measurements (4.00%)
  2 (2.00%) high mild
  2 (2.00%) high severe
concurrent_batch_generates/Num Concurrent Requests/8
                        time:   [344.48 µs 345.89 µs 347.39 µs]
                        change: [-7.4563% -5.4143% -3.5829%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 7 outliers among 100 measurements (7.00%)
  1 (1.00%) low severe
  1 (1.00%) low mild
  2 (2.00%) high mild
  3 (3.00%) high severe
concurrent_batch_generates/Num Concurrent Requests/10
                        time:   [377.93 µs 381.92 µs 389.11 µs]
                        change: [-1.3829% +2.0156% +6.8516%] (p = 0.42 > 0.05)
                        No change in performance detected.
Found 10 outliers among 100 measurements (10.00%)
  6 (6.00%) high mild
  4 (4.00%) high severe
concurrent_batch_generates/Num Concurrent Requests/20
                        time:   [517.85 µs 541.65 µs 573.03 µs]
                        change: [-3.7601% -0.4496% +3.2736%] (p = 0.82 > 0.05)
                        No change in performance detected.
Found 11 outliers among 100 measurements (11.00%)
  6 (6.00%) high mild
  5 (5.00%) high severe
Benchmarking concurrent_batch_generates/Num Concurrent Requests/50: Warming up for 3.0000 s
Warning: Unable to complete 100 samples in 5.0s. You may wish to increase target time to 5.1s, enable flat sampling, or reduce sample count to 60.
concurrent_batch_generates/Num Concurrent Requests/50
                        time:   [1.0024 ms 1.0059 ms 1.0105 ms]
                        change: [+0.1373% +0.9971% +1.9716%] (p = 0.03 < 0.05)
                        Change within noise threshold.
Found 25 outliers among 100 measurements (25.00%)
  3 (3.00%) low severe
  2 (2.00%) low mild
  3 (3.00%) high mild
  17 (17.00%) high severe
concurrent_batch_generates/Num Concurrent Requests/100
                        time:   [2.0011 ms 2.0036 ms 2.0066 ms]
                        change: [-0.8395% -0.3760% -0.0200%] (p = 0.07 > 0.05)
                        No change in performance detected.
Found 13 outliers among 100 measurements (13.00%)
  3 (3.00%) low mild
  3 (3.00%) high mild
  7 (7.00%) high severe
concurrent_batch_generates/Num Concurrent Requests/200
                        time:   [4.6446 ms 4.7708 ms 4.9470 ms]
                        change: [+2.5601% +5.2743% +9.0748%] (p = 0.00 < 0.05)
                        Performance has regressed.
Found 9 outliers among 100 measurements (9.00%)
  2 (2.00%) high mild
  7 (7.00%) high severe
concurrent_batch_generates/Num Concurrent Requests/500
                        time:   [12.167 ms 12.332 ms 12.559 ms]
                        change: [+0.1147% +1.4656% +3.3595%] (p = 0.07 > 0.05)
                        No change in performance detected.
Found 6 outliers among 100 measurements (6.00%)
  1 (1.00%) high mild
  5 (5.00%) high severe
concurrent_batch_generates/Num Concurrent Requests/1000
                        time:   [24.657 ms 24.909 ms 25.318 ms]
                        change: [-0.6338% +0.7789% +2.4971%] (p = 0.42 > 0.05)
                        No change in performance detected.
Found 19 outliers among 100 measurements (19.00%)
  8 (8.00%) low mild
  8 (8.00%) high mild
  3 (3.00%) high severe
```
</details>



| Concurrent Requests | Latency | Theoretical Throughput |
|-------------------------|-------------|----------------------|
| 2 | 279μs | 7,166 req/sec |
| 4 | 301μs | 13,300 req/sec |
| 6 | 336μs | 17,886 req/sec |
| 8 | 346μs | 23,128 req/sec |
| 10 | 382μs | 26,180 req/sec |
| 20 | 542μs | 36,920 req/sec |
| 50 | 1.01ms | 49,700 req/sec |
| 100 | 2.00ms | 49,900 req/sec |
| 200 | 4.77ms | 42,000 req/sec |
| 500 | 12.33ms | 40,500 req/sec |
| 1,000 | 24.91ms | 40,000 req/sec |

> [!NOTE] 
> The `concurrent_batch_generates` bench submits concurrent batch requests for 100 snowflake IDs each. This means the theoretical ID
> throughput is 100x the requests per second. 

---


### **GET** `/health`
---
This image also supports a health check endpoint that will return a `200 OK` if the server is running

---