ARG BASE_IMAGE=ubuntu:22.04
FROM ${BASE_IMAGE}
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
    && apt-get install -y --no-install-recommends openssh-server tmux vim-tiny \
    && rm -rf /var/lib/apt/lists/*
