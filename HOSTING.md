# Hosting Guide

## Getting Started
Since `snowflake-id-worker` is a [Docker image](https://docs.docker.com/get-started/docker-concepts/the-basics/what-is-an-image/), it can be
interacted with just like any other image. To run a single snowflake ID worker, you can use either a
[`docker`](https://docs.docker.com/reference/cli/docker/) command, or a
[`docker compose up`](https://docs.docker.com/reference/cli/docker/compose/up/) command with a valid
[`compose.yaml`](https://docs.docker.com/compose/intro/compose-application-model/#the-compose-file)

### via `docker`:
```bash
docker run -p 8080:8080 ghcr.io/ayubun/snowflake-id-worker:0
```
### via `docker compose up` / `compose.yaml`:
```yml
version: '3.8'

services:
  snowflake-id-worker:
    image: ghcr.io/ayubun/snowflake-id-worker:0
    ports:
      - 8080:8080
```

> [!NOTE] 
> The HTTP API is registered on port 8080 within the image

> [!IMPORTANT]
> The above commands will pull the `snowflake-id-worker:0` image, which auto-updates upon bugfix and minor version changes. 
> If you want to use a more static image version, you can supply one instead. Examples:
> - `ghcr.io/ayubun/snowflake-id-worker:0.3`
> - `ghcr.io/ayubun/snowflake-id-worker:0.3.1`
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
```

> [!IMPORTANT] 
> The `EPOCH` environment variable must be consistent across all workers
