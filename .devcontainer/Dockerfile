FROM ubuntu:latest

RUN apt-get update && apt-get install -y \
    sudo \
    lsb-release wget software-properties-common gnupg openssh-client \
    build-essential \
    python3 python3-pip \
    # For Rust compiler's build
    libssl-dev pkg-config \
    ;

# FIXME: Define these as features.

# Source: https://apt.llvm.org/
RUN bash -c "$(wget -O - https://apt.llvm.org/llvm.sh)"

ARG CMAKE_BIN_URL
RUN wget -qO- $CMAKE_BIN_URL \
    | tar --strip-components=1 -xz -C /usr/local

COPY --from=ghcr.io/sfu-rsl/z3_dist:4.11.2 /home/dist /usr

RUN wget https://souffle-lang.github.io/ppa/souffle-key.public -O /usr/share/keyrings/souffle-archive-keyring.gpg \
    && echo "deb [signed-by=/usr/share/keyrings/souffle-archive-keyring.gpg] https://souffle-lang.github.io/ppa/ubuntu/ stable main" \
    | tee /etc/apt/sources.list.d/souffle.list \
    && apt update && apt install -y souffle \
    ;

RUN useradd -m -s /bin/bash vscode \
    && echo 'vscode ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/vscode
USER vscode
