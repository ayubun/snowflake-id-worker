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

**RESPONSES:**

<details>
<summary>Status codes returned by <strong>POST</strong> <code>/generate</code></summary>

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

The benchmarks send requests in-process to a single-threaded tokio runtime (plus the worker's own generator thread), so results are not
skewed by how many cores the machine has. You can run them yourself with `cargo bench`. The results below are from an AMD EPYC 9B45:

| Request | Latency | Throughput |
|--|--|--|
| 1 request, single ID | 4.14µs | 242K IDs/sec |
| 1 request, `{"count":100}` | 24.4µs | 4.10M IDs/sec |
| 1 request, `{"count":10000}` | 2.44ms | 4.10M IDs/sec |
| 100 concurrent requests, single ID each | 137µs | 730K IDs/sec |

Batches of 100 or more saturate the generator's cap of 4096 IDs per millisecond. Single ID requests spend most of their time on the round trip
between the HTTP handler and the generator thread, which is why sending them concurrently helps but still lands well short of batching.

<details>
<summary><strong>Raw</strong> <code>cargo bench</code> <strong>output:</strong></summary>

```
starting snowflake-id-worker with WORKER_ID: 0, DATA_CENTER_ID: 0, and EPOCH: 0
generate/count/1        time:   [4.0615 µs 4.1355 µs 4.2125 µs]
                        thrpt:  [237.39 Kelem/s 241.81 Kelem/s 246.21 Kelem/s]
Found 4 outliers among 100 measurements (4.00%)
  3 (3.00%) high mild
  1 (1.00%) high severe
generate/count/10       time:   [4.5952 µs 4.6462 µs 4.6971 µs]
                        thrpt:  [2.1290 Melem/s 2.1523 Melem/s 2.1762 Melem/s]
Found 4 outliers among 100 measurements (4.00%)
  3 (3.00%) high mild
  1 (1.00%) high severe
generate/count/100      time:   [24.402 µs 24.418 µs 24.439 µs]
                        thrpt:  [4.0919 Melem/s 4.0953 Melem/s 4.0981 Melem/s]
Found 13 outliers among 100 measurements (13.00%)
  3 (3.00%) low severe
  1 (1.00%) low mild
  1 (1.00%) high mild
  8 (8.00%) high severe
generate/count/1000     time:   [243.95 µs 244.19 µs 244.44 µs]
                        thrpt:  [4.0910 Melem/s 4.0951 Melem/s 4.0993 Melem/s]
Found 14 outliers among 100 measurements (14.00%)
  5 (5.00%) low severe
  2 (2.00%) low mild
  2 (2.00%) high mild
  5 (5.00%) high severe
generate/count/10000    time:   [2.4377 ms 2.4413 ms 2.4451 ms]
                        thrpt:  [4.0898 Melem/s 4.0961 Melem/s 4.1023 Melem/s]
generate/count/100000   time:   [24.425 ms 24.456 ms 24.488 ms]
                        thrpt:  [4.0836 Melem/s 4.0890 Melem/s 4.0941 Melem/s]

starting snowflake-id-worker with WORKER_ID: 0, DATA_CENTER_ID: 0, and EPOCH: 0
concurrent_single_generates/requests/10
                        time:   [13.084 µs 13.212 µs 13.368 µs]
                        thrpt:  [748.04 Kelem/s 756.90 Kelem/s 764.27 Kelem/s]
Found 12 outliers among 100 measurements (12.00%)
  4 (4.00%) low mild
  2 (2.00%) high mild
  6 (6.00%) high severe
concurrent_single_generates/requests/100
                        time:   [136.44 µs 136.90 µs 137.42 µs]
                        thrpt:  [727.68 Kelem/s 730.45 Kelem/s 732.91 Kelem/s]
Found 9 outliers among 100 measurements (9.00%)
  1 (1.00%) low severe
  1 (1.00%) low mild
  3 (3.00%) high mild
  4 (4.00%) high severe
```
</details>

---


### **GET** `/health`
---
This image also supports a health check endpoint that will return a `200 OK` if the server is running

---
