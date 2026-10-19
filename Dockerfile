# The suture command in a small image, for pods (guide chapter 22).
#
#   docker build -t suture .
#
# A program goes in /app and its state on a volume at /data:
#
#   FROM suture
#   COPY . /app

ARG RUST_VERSION=1.89

FROM rust:${RUST_VERSION}-slim-trixie AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p suture-cli \
 && cp target/release/suture /usr/local/bin/suture

# What the command needs to run: curl to reach adapters and models, and
# tini to pass on the stop signal and reap the curl processes.
FROM debian:trixie-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl tini \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home-dir /data --shell /usr/sbin/nologin suture \
 && mkdir -p /app /data \
 && chown suture /data
USER 10001
WORKDIR /data
VOLUME /data
EXPOSE 7878
HEALTHCHECK --interval=10s --timeout=3s --start-period=5m CMD curl -fsS http://127.0.0.1:7878/health || exit 1
ENTRYPOINT ["/usr/bin/tini", "--", "suture"]
CMD ["serve", "/app", "--state", "/data", "--listen", "0.0.0.0:7878"]

FROM runtime
COPY --from=build /usr/local/bin/suture /usr/local/bin/suture
