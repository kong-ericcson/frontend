FROM kong:3.9
USER root
RUN apt-get update -qq && \
    apt-get install -y -qq --no-install-recommends curl ca-certificates > /dev/null 2>&1 && \
    luarocks install kong-phantom-token 2.0.1 && \
    apt-get purge -y curl && apt-get autoremove -y -qq && rm -rf /var/lib/apt/lists/*
USER kong
