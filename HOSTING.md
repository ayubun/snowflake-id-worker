# Hosting Guide

## Getting Started
Since `snowflake-id-worker` is a [Docker image](https://docs.docker.com/get-started/docker-concepts/the-basics/what-is-an-image/), it can be
interacted with just like any other image. To run a single snowflake ID worker, you can use either a
[`docker`](https://docs.docker.com/reference/cli/docker/) command, or a
[`docker compose up`](https://docs.docker.com/reference/cli/docker/compose/up/) command with a valid
[`compose.yaml`](https://docs.docker.com/compose/intro/compose-application-model/#the-compose-file)

### via `docker`:
```bash
docker run -p 80:80 ghcr.io/ayubun/snowflake-id-worker
```
### via `docker compose up` / `compose.yaml`:
```yml
version: '3.8'

services:
  snowflake-id-worker:
    image: ghcr.io/ayubun/snowflake-id-worker
    ports:
      - 80:80
```

> [!NOTE] 
> The HTTP API is registered on port 80 within the image

## Basic Multi-Worker Example

Using a [`compose.yaml`](https://docs.docker.com/compose/intro/compose-application-model/#the-compose-file), a multi-worker cluster might
look like such:
```yml
version: '3.8'

services:
  snowflake-id-worker-0:
    image: ghcr.io/ayubun/snowflake-id-worker
    restart: always
    ports:
      - 80:80
    environment:
      - WORKER_ID=0
      - EPOCH=1420070400000
  snowflake-id-worker-1:
    image: ghcr.io/ayubun/snowflake-id-worker
    restart: always
    ports:
      # If you are hosting multiple workers on a single machine, you 
      # will need to increment the effective port to avoid conflicts
      - 81:80
    environment:
      - WORKER_ID=1
      - EPOCH=1420070400000
```

> [!IMPORTANT] 
> The `EPOCH` environment variable must be consistent across all workers
