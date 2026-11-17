ARG BASE_IMAGE=ubuntu:22.04
FROM ${BASE_IMAGE}

ENV DEBIAN_FRONTEND=noninteractive
RUN rm -f /etc/apt/sources.list.d/cuda*.list \
    && apt-get update \
    && apt-get install -y --no-install-recommends \
      build-essential pkg-config libwebkit2gtk-4.1-dev libssl-dev \
      libxdo-dev libayatana-appindicator3-dev librsvg2-dev libvte-2.91-dev fonts-noto-cjk openssh-client xvfb xauth \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /workspace
