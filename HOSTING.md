# Hosting Guide

## Getting Started
Since `snowflake-id-worker` is a [Docker image](https://docs.docker.com/get-started/docker-concepts/the-basics/what-is-an-image/), it can be
interacted with just like any other image. To run a single snowflake ID worker, you can use either a
[`docker`](https://docs.docker.com/reference/cli/docker/) command, or a
[`docker compose up`](https://docs.docker.com/reference/cli/docker/compose/up/) command with a valid
[`compose.yaml`](https://docs.docker.com/compose/intro/compose-application-model/#the-compose-file)

### via `docker`:
```bash
docker run --cpus=0.5 --memory=64m -p 8080:8080 ghcr.io/ayubun/snowflake-id-worker:0
```
### via `docker compose up` / `compose.yaml`:
```yml
version: '3.8'

services:
  snowflake-id-worker:
    image: ghcr.io/ayubun/snowflake-id-worker:0
    ports:
      - 8080:8080
    deploy:
      resources:
        limits:
          cpus: '0.5'
          memory: 64M
```

> [!NOTE] 
> The HTTP API is registered on port 8080 within the image

> [!IMPORTANT]
> The above commands will pull the `snowflake-id-worker:0` image, which auto-updates upon bugfix and minor version changes. 
> If you want to use a more static image version, you can supply one instead. Examples:
> - `ghcr.io/ayubun/snowflake-id-worker:0.4`
> - `ghcr.io/ayubun/snowflake-id-worker:0.4.1`
>
> Alternatively, you can live on the edge and use the `latest` tag >:D (not supplying a tag will default to `latest`)

## Basic Multi-Worker Example

Using a [`compose.yaml`](https://docs.docker.com/compose/intro/compose-application-model/#the-compose-file), a multi-worker cluster might
look like such:
```yml
version: '3.8'

services:
  snowflake-id-worker-0:
    image: ghcr.io/ayubun/snowflake-id-worker:0
    restart: always
    ports:
      - 8080:8080
    environment:
      - WORKER_ID=0
      - EPOCH=1420070400000
    deploy:
      resources:
        limits:
          cpus: '0.5'
          memory: 64M
  snowflake-id-worker-1:
    image: ghcr.io/ayubun/snowflake-id-worker:0
    restart: always
    ports:
      # If you are hosting multiple workers on a single machine, you 
      # will need to use a different effective port to avoid conflicts
      - 9090:8080
    environment:
      - WORKER_ID=1
      - EPOCH=1420070400000
    deploy:
      resources:
        limits:
          cpus: '0.5'
          memory: 64M
```

> [!IMPORTANT] 
> The `EPOCH` environment variable must be consistent across all workers

## Recommended Resources

Half a CPU and 64 MiB per worker, as in the examples above. Half a CPU is enough to reach the generator's cap of 4096 IDs per millisecond
with batch requests, so more CPU only speeds up single ID requests. Peak memory under load is about 12 MiB with the default `MAX_BATCH_SIZE`

### via k8s `resources`:
```yml
resources:
  requests:
    cpu: 500m
    memory: 64Mi
  limits:
    cpu: 500m
    memory: 64Mi
```

> [!NOTE]
> If you need more throughput, add workers with distinct `WORKER_ID`s rather than CPU, since each worker brings its own 4096 IDs per millisecond.
> Raise the memory limit if you raise `MAX_BATCH_SIZE`
