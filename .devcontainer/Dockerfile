FROM mcr.microsoft.com/devcontainers/cpp:2-ubuntu24.04

ARG REINSTALL_CMAKE_VERSION_FROM_SOURCE="none"

COPY ./reinstall-cmake.sh /tmp/

RUN if [ "${REINSTALL_CMAKE_VERSION_FROM_SOURCE}" != "none" ]; then \
        chmod +x /tmp/reinstall-cmake.sh && /tmp/reinstall-cmake.sh ${REINSTALL_CMAKE_VERSION_FROM_SOURCE}; \
    fi \
    && rm -f /tmp/reinstall-cmake.sh

# Native deps for blink:
#   libav*        - video encode + MKV mux (linked via ffmpeg-next)
#   libgtk-3      - GTK main loop (required by tray-icon on Linux)
#   libayatana-*  - system tray on Ubuntu 22+/24.04
#   libxcb/xrandr - X11 headers used by xcap (screenshot capture)
#   ffmpeg        - runtime convenience for manual playback / debugging
RUN apt-get update && export DEBIAN_FRONTEND=noninteractive \
    && apt-get -y install --no-install-recommends \
        pkg-config \
        libavformat-dev libavcodec-dev libavutil-dev libswscale-dev \
        libavfilter-dev libavdevice-dev \
        libgtk-3-dev libayatana-appindicator3-dev \
        libxcb1-dev libxrandr-dev libxfixes-dev libxext-dev libxdo-dev \
        libssl-dev \
        ffmpeg mpv \
    && rm -rf /var/lib/apt/lists/*

# Rust stable toolchain for the vscode user
USER vscode
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain stable --profile minimal \
    && /home/vscode/.cargo/bin/rustup component add rustfmt clippy
ENV PATH="/home/vscode/.cargo/bin:${PATH}"
USER root
