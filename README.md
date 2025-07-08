# Summary
`snowflake-id-worker` is a [Docker image](https://docs.docker.com/get-started/docker-concepts/the-basics/what-is-an-image/) published to
[Github's container repository](https://ghcr.io/). It allows API callers to generate unique [snowflake IDs](https://en.wikipedia.org/wiki/Snowflake_ID) across a
distributed system using Twitter's snowflake algorithm.

Callers can choose to generate a singular snowflake ID or supply a count in the JSON body to
generate a batch (˶ᵔ ᵕ ᵔ˶) The worker is written in Rust to optimize for performance~

## API Spec
The `snowflake-id-worker` serves an HTTP `POST /generate` endpoint to a port of the host's choice. This endpoint can be used to generate
one or many snowflake IDs

> [!NOTE]
> The API will always return a list so that consistent behaviour can be expected, even if only a singular snowflake ID is requested

If a `count` is specified in the request body (i.e. `{"count":10}`), the endpoint will return a batch of snowflake IDs with the requested count:
![`POST /generate` with populated request body](assets/generate-example-populated-body.png)
If a `count` is **not** specified in the request body, one snowflake ID will be returned:
![`POST /generate` with empty request body](assets/generate-example-empty-body.png)

### Health checking
This image also supports a `GET /health` endpoint, which will return a `200 OK` if the server is running

# Hosting

## Getting Started
Since `snowflake-id-worker` is a [Docker image](https://docs.docker.com/get-started/docker-concepts/the-basics/what-is-an-image/), it can be
interacted with just like any other image. To run a single snowflake ID worker, you can use either a
[`docker`](https://docs.docker.com/reference/cli/docker/) command, or a
[`docker compose up`](https://docs.docker.com/reference/cli/docker/compose/up/) command with a valid
[`compose.yaml`](https://docs.docker.com/compose/intro/compose-application-model/#the-compose-file)

> [!NOTE]
> `DATA_CENTER_ID` and `WORKER_ID` will each default to `0` if their environment variables aren't present. More on the purpose of these
> later~

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

## Scaling
In order to scale to greater than one worker, you will need to supply a `WORKER_ID` environment variable to differentiate between the workers:
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
  snowflake-id-worker-1:
    image: ghcr.io/ayubun/snowflake-id-worker
    restart: always
    ports:
      # If you are hosting multiple workers on a single machine, you 
      # will need to increment the effective port to avoid conflicts
      - 81:80
    environment:
      - WORKER_ID=1
```
This will start two Docker images that can generate snowflake IDs in parallel without any conflicts, since the `WORKER_ID` is unique per worker.

You can also leverage a `DATA_CENTER_ID` to differentiate even further. As long as each worker has a unique combination of a `DATA_CENTER_ID`
and a `WORKER_ID`, the snowflake ID generation of the cluster will remain unique.

> [!WARNING]
> This means that if two workers share the **same** `DATA_CENTER_ID` and `WORKER_ID`, **the uniqueness of the IDs that they generate cannot
> be guaranteed**

> [!IMPORTANT]
> `WORKER_ID` and `DATA_CENTER_ID` are handled as `u8`s. Therefore, they must each be between `0` and `31`

### Non-UNIX Epochs
`snowflake-id-worker` also supports non-UNIX epochs. For example, Discord
[documents](https://discord.com/developers/docs/reference#snowflakes) using a custom epoch of `1420070400000`. Supplying an `EPOCH`
environment variable on each worker allows you to overwrite the UNIX epoch:
```yml
version: '3.8'

services:
  snowflake-worker:
    image: ghcr.io/ayubun/snowflake-id-worker
    restart: always
    ports:
      - 80:80
    environment:
      # Discord's snowflake epoch
      - EPOCH=1420070400000
```
