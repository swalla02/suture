# 22. Containers

Chapter 20 runs several pods over one state directory. The repository's `Dockerfile` packages the `suture` command into a small image, so the pods can run as containers, for instance on Kubernetes.

## The image

```sh
docker build -t suture .
```

The image holds the `suture` command, `curl` (adapters and models are reached through it, chapters 17 and 15) and `tini`, which passes the stop signal on and reaps the `curl` processes. It runs as the user `10001`, not as root. By default it serves the program in `/app`, keeps the state in the volume `/data`, and listens on port 7878 on every interface:

```text
suture serve /app --state /data --listen 0.0.0.0:7878
```

A program's image adds its directory, with its `adapters.json` and, if it uses one, its `models.json`:

```dockerfile
FROM suture
COPY examples/tickets /app
```

The program is compiled when the container starts, so a program with errors stops the container with the errors in its log.

Keys stay out of the image. `adapters.json` and `models.json` name environment variables (`key_env`, chapters 15 and 17), and the container gets them from the platform's secrets. So does `SUTURE_OPERATOR_TOKEN`.

## Pods in containers

Each container is one pod. Give each a name with `--pod`, and the same volume at `/data`:

```sh
docker volume create tickets
docker run -d --name tickets-a -v tickets:/data -e SUTURE_OPERATOR_TOKEN tickets:1 \
  serve /app --state /data --listen 0.0.0.0:7878 --pod a
docker run -d --name tickets-b -v tickets:/data -e SUTURE_OPERATOR_TOKEN tickets:1 \
  serve /app --state /data --listen 0.0.0.0:7878 --pod b
```

The image's health check asks `GET /health` every 10 seconds, on port 7878. If a container listens on another port, give it a health check of its own. A pod replays the whole log before it listens (chapter 17), so the check allows 5 minutes to start.

## Stopping

`suture serve` stops on `SIGTERM` or `SIGINT`. It finishes the request it is on, fires no more timers and posts nothing more, and leaves the pods, so its units go to the other pods at once instead of 10 seconds later, then exits with status 0:

```text
stopped; pod a left
```

A request can take up to 15 seconds: 5 to read it, and 10 to post to a slow adapter. So give a container more than that to stop, as in `docker stop --time 30`. A pod that is killed instead, or that stops on an error, leaves its units for the others 10 seconds later, when its beat runs out. It also leaves the pods when it stops on an error after joining them.

## Kubernetes

`deploy/k8s/tickets.yaml` runs the tickets example as a StatefulSet of two pods:

- every pod mounts one `ReadWriteMany` volume at `/data`, whose file system must support `flock` across nodes (chapter 20);
- each pod is named after its Kubernetes pod (`--pod $(POD_NAME)`, so `tickets-0` and `tickets-1`);
- `/health` is the startup, readiness and liveness probe, and the startup probe gives a long replay up to 5 minutes;
- the pods get 30 seconds to stop;
- `SUTURE_OPERATOR_TOKEN` comes from the secret `tickets-operator`;
- the service `tickets` must be reachable only by the gateway, since the pods trust the `Suture-Caller` header it sets (chapter 17).

A new version is a new image. Roll it out as chapter 20 says: the first pod with the new version deploys it, and the others pick it up from the log and are then restarted with the new image too. A StatefulSet's rolling update does this one pod at a time.
