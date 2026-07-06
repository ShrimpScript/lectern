# Reproducible sandbox for the Lectern benchmark — a pinned environment so runs
# are comparable across machines. The default runner isolation is a fresh temp
# git repo per run; this image is for external, fully-pinned reproduction.
#
#   docker build -t lectern-bench -f bench/Dockerfile .
#   docker run --rm -v "$PWD":/repo -w /repo lectern-bench \
#     python3 bench/runner.py --backend mock
#
# For the real $0 study you also need opencode configured with free models
# (see https://opencode.ai and bench/METHODOLOGY.md); mount your opencode auth
# into the container, e.g. -v "$HOME/.local/share/opencode":/root/.local/share/opencode.
FROM rust:1-slim-bookworm

RUN apt-get update && apt-get install -y --no-install-recommends \
      git python3 curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Node (for opencode + Lectern's TUI); pinned major for stability.
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /repo
# Build the CLI once so the runner has target/debug/lectern; if the source isn't
# present at build time this is a no-op and you can mount a prebuilt binary via
# LECTERN_BIN instead.
COPY . /repo
RUN cargo build -p lectern || true

CMD ["python3", "bench/runner.py", "--backend", "mock"]
