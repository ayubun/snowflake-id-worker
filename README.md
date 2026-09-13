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
| `WORKER_ID` | `0` | `0` to `31`, or "`FROM_HOSTNAME`" | An identifier for the given worker. Setting this value to "`FROM_HOSTNAME`" will try to parse the worker ID from the end of the hostname. This feature is for workers being run in k8s StatefulSets |
| `DATA_CENTER_ID` | `0` | `0` to `31` | An identifier for the location that a given set of workers are running on |
| `EPOCH` | UNIX Epoch | `u64` | An optional environment variable that allows hosts to use a custom epoch. For example, Discord uses a custom epoch of `1420070400000`. Epochs in the future are rejected, and a more recent epoch leaves more of the 41-bit timestamp range available |
| `PORT` | `8080` | `u16` | The port that the HTTP API listens to requests from. If you are using the snowflake-id-worker image, modifying this environment variable may also require adding a [Docker port forward](https://docs.docker.com/get-started/docker-concepts/running-containers/publishing-ports/) |
| `MAX_BATCH_SIZE` | `100000` | `1` to `10000000` | The largest `count` accepted by `POST /generate`. Requests with a larger `count` will return a `400 Bad Request` |

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

<details>
<summary>Below are the status codes returned by this endpoint</summary>

| Status | When |
|--|--|
| `200 OK` | Success. The body is a JSON array of snowflake IDs |
| `400 Bad Request` | The body is malformed JSON, contains an unknown field, or has a `count` that is not a positive integer within `MAX_BATCH_SIZE` |
| `411 Length Required` | The request has no `Content-Length` header. With curl, pass `-d ''` to request a single snowflake ID |
| `413 Payload Too Large` | The request body is larger than 1 KiB |
| `429 Too Many Requests` | The generation queue is full. The response includes a `Retry-After` header |

</details>

> [!NOTE]
> A single worker generates at most 4096 snowflake IDs per millisecond. Requests beyond that queue up on the worker's generator thread, and once the queue
> is full they receive a `429`. If the system clock steps backwards, the worker advances its logical time until wall time catches up

### Benchmarks & Optimization Notes
---

> [!IMPORTANT]
> Generating snowflake IDs in batches is far more efficient than generating them one at a time. If high throughput per worker is
> essential for your use-case, you will want to factor batching into the design of your clients.

The benchmarks send requests in-process to a single-threaded tokio runtime (plus the worker's own generator thread), and on Linux the whole
bench process is pinned to one CPU so the HTTP thread and the generator thread share a core like they do in a deployed worker. Set `BENCH_CPUS`
(for example `BENCH_CPUS=0,1`) to pin differently, and run them yourself with `cargo bench`. The results below are from one core of an AMD EPYC 9B45:

| Request | Latency | Throughput |
|--|--|--|
| 1 request, single ID | 3.03µs | 330K IDs/sec |
| 1 request, `{"count":100}` | 24.5µs | 4.08M IDs/sec |
| 1 request, `{"count":10000}` | 2.45ms | 4.08M IDs/sec |
| 100 concurrent requests, single ID each | 201µs | 498K IDs/sec |

Batches of 100 or more saturate the generator's cap of 4096 IDs per millisecond. Single ID requests spend most of their time on the round trip
between the HTTP handler and the generator thread, which is why sending them concurrently helps but still lands well short of batching.
See [HOSTING](./HOSTING.md#recommended-resources) for what this means when sizing a deployment.

<details>
<summary><strong>Raw</strong> <code>cargo bench</code> <strong>output:</strong></summary>

```
bench pinned to cpus [0]
starting snowflake-id-worker with WORKER_ID: 0, DATA_CENTER_ID: 0, and EPOCH: 0
generate/count/1        time:   [2.9730 µs 3.0258 µs 3.0919 µs]
                        thrpt:  [323.43 Kelem/s 330.49 Kelem/s 336.36 Kelem/s]
Found 15 outliers among 100 measurements (15.00%)
  3 (3.00%) high mild
  12 (12.00%) high severe
generate/count/10       time:   [3.7480 µs 3.7905 µs 3.8417 µs]
                        thrpt:  [2.6030 Melem/s 2.6381 Melem/s 2.6681 Melem/s]
Found 8 outliers among 100 measurements (8.00%)
  8 (8.00%) high severe
generate/count/100      time:   [24.439 µs 24.492 µs 24.558 µs]
                        thrpt:  [4.0721 Melem/s 4.0830 Melem/s 4.0919 Melem/s]
Found 24 outliers among 100 measurements (24.00%)
  5 (5.00%) low severe
  1 (1.00%) low mild
  1 (1.00%) high mild
  17 (17.00%) high severe
generate/count/1000     time:   [244.64 µs 245.31 µs 246.17 µs]
                        thrpt:  [4.0622 Melem/s 4.0765 Melem/s 4.0877 Melem/s]
Found 9 outliers among 100 measurements (9.00%)
  2 (2.00%) low severe
  1 (1.00%) low mild
  6 (6.00%) high severe
generate/count/10000    time:   [2.4429 ms 2.4484 ms 2.4545 ms]
                        thrpt:  [4.0742 Melem/s 4.0843 Melem/s 4.0935 Melem/s]
Found 2 outliers among 100 measurements (2.00%)
  2 (2.00%) high mild
generate/count/100000   time:   [24.539 ms 24.612 ms 24.699 ms]
                        thrpt:  [4.0488 Melem/s 4.0631 Melem/s 4.0751 Melem/s]
Found 5 outliers among 100 measurements (5.00%)
  1 (1.00%) high mild
  4 (4.00%) high severe

starting snowflake-id-worker with WORKER_ID: 0, DATA_CENTER_ID: 0, and EPOCH: 0
concurrent_single_generates/requests/10
                        time:   [19.533 µs 19.717 µs 19.951 µs]
                        thrpt:  [501.23 Kelem/s 507.17 Kelem/s 511.97 Kelem/s]
Found 11 outliers among 100 measurements (11.00%)
  3 (3.00%) high mild
  8 (8.00%) high severe
concurrent_single_generates/requests/100
                        time:   [192.24 µs 200.68 µs 209.45 µs]
                        thrpt:  [477.44 Kelem/s 498.31 Kelem/s 520.19 Kelem/s]
Found 19 outliers among 100 measurements (19.00%)
  9 (9.00%) high mild
  10 (10.00%) high severe
```
</details>

---


### **GET** `/health`
---
This image also supports a health check endpoint that will return a `200 OK` if the server is running

---
