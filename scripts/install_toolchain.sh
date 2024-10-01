#!/usr/bin/bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: <text>
# Copyright(c) 2026 Liebherr-Digital Development Center GmbH
# Written by Thomas Witte <thomas.witte@liebherr.com>
# </text>

# Installs toolchain and various tools.

# Install versions
RUST_VERSION=1.75.0
CROSS_VERSION=0.2.5
MDBOOK_VERSION=0.4.40
FD_VERSION=10.2.0
CLIFF_VERSION=2.6.0
LYCHEE_VERSION=0.15.1
BINDGEN_VERSION=0.70.1

usage() {
    echo "Usage: $0 [-s | --system] [-u | --update]"
    echo
    echo "Options:"
    echo "  -s, --system       Install system deps"
    echo "  -u, --update       Update tool versions"
    echo "  -h, --help         Display this help message"
    echo
}

if ! OPTS=$(getopt -o "suh" -l "system,update,help" -n "$0" -- "$@"); then
    usage
    exit 1
fi
eval set -- "$OPTS"

install_toolchain() {
    curl -LsSf --proto '=https' --tlsv1.2 https://sh.rustup.rs | sh -s -- -q -y --default-toolchain=$RUST_VERSION

    cargo install --locked -q mdbook@$MDBOOK_VERSION
    cargo install --locked -q fd-find@$FD_VERSION
    cargo install --locked -q git-cliff@$CLIFF_VERSION
    cargo install --locked -q cross@$CROSS_VERSION
    cargo install --locked -q lychee@$LYCHEE_VERSION
    cargo install --locked -q bindgen-cli@$BINDGEN_VERSION

    curl -LsSf --proto '=https' --tlsv1.2 https://astral.sh/uv/install.sh | sh
    uv venv
    uv pip install "qa @ ."

    # For interactive development.
    git config --get include.path ../.gitconfig || git config --add include.path ../.gitconfig
    uv pip install "qa[tools] @ ."
    pre-commit install --hook-type commit-msg --hook-type pre-commit
    echo "Dont forget to source: source .venv/bin/activate"
}

install_system_deps() {
    apt-get update \
        && apt-get install -y \
        binfmt-support \
        build-essential \
        cmake \
        libclang1 \
        curl \
        git \
        g++-arm-linux-gnueabihf \
        g++-mingw-w64 \
        libc6-dev-armhf-cross \
        qemu-user-static \
        pkg-config \
        libssl-dev
}

update() {
    echo RUST_VERSION="$(gh release view --repo github.com/rust-lang/rust --json tagName --jq '.tagName')"
    echo CROSS_VERSION="$(gh release view --repo github.com/cross-rs/cross --json tagName --jq '.tagName[1:]')"
    echo MDBOOK_VERSION="$(gh release view --repo github.com/rust-lang/mdBook --json tagName --jq '.tagName[1:]')"
    echo FD_VERSION="$(gh release view --repo github.com/sharkdp/fd --json tagName --jq '.tagName[1:]')"
    echo CLIFF_VERSION="$(gh release view --repo github.com/orhun/git-cliff --json tagName --jq '.tagName[1:]')"
    echo LYCHEE_VERSION="$(gh release view --repo github.com/lycheeverse/lychee --json tagName --jq '.tagName[1:]')"
    echo BINDGEN_VERSION="$(gh release view --repo github.com/rust-lang/rust-bindgen --json tagName --jq '.tagName[1:]')"
}


while true; do
    case "$1" in
        -s|--system)
            install_system_deps
            exit 0
            ;;
        -u|--update)
            update
            exit 0
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        --)
            shift
            break
            ;;
        *)
            echo "Unknown option: $1"
            usage
            exit 1
            ;;
    esac
done


install_toolchain
