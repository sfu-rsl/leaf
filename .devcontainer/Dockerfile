FROM mcr.microsoft.com/devcontainers/base:debian

RUN apt-get update && apt-get install -y \
    # For Rust compiler's build
    libssl-dev pkg-config \
    ;

# FIXME: Define these as features.
# NOTE: LLVM feature provided by the community does not work correctly at this moment.
RUN bash -c "$(wget -O - https://apt.llvm.org/llvm.sh)" \
    && apt-get clean -y && rm -rf /var/lib/apt/lists/*

# Souffle for orchestrators (optional)
RUN wget --no-verbose https://github.com/souffle-lang/souffle/releases/download/2.5/x86_64-ubuntu-2404-souffle-2.5-Linux.deb -O /tmp/souffle.deb \
    && apt-get update && apt-get install -y libsqlite3-dev /tmp/souffle.deb \
    && apt-get clean -y && rm -rf /var/lib/apt/lists/*
