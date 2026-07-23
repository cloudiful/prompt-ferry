FROM debian:trixie-slim AS runtime

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt/lists,sharing=locked \
    apt-get update \
    && apt-get install -y --no-install-recommends bash ca-certificates tini \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --chmod=755 ci-image-input/prompt-ferry /usr/local/bin/prompt-ferry
COPY ci-image-input/frontend-dist /app/frontend/dist
COPY --chmod=755 docker/start-prompt-ferry.sh /usr/local/bin/start-prompt-ferry.sh

EXPOSE 8787 80
ENTRYPOINT ["tini", "--", "/usr/local/bin/start-prompt-ferry.sh"]
CMD ["relay"]
