<!--
SPDX-License-Identifier: CC-BY-SA-4.0
SPDX-FileCopyrightText: <text>
Copyright(c) 2026 Liebherr-Digital Development Center GmbH
Written by Thomas Witte <thomas.witte@liebherr.com>
</text>
-->

# ldc_iot_updated

sw update service that integrates swupdate, hawkbit, and additional
requirements, e.g. user consent prompts

## Getting Started

The project can be built and tested using the *cargo* build tool.

```sh
cargo build
cargo test
```

Install cross toolchains.

```sh
cargo install cross
```

## Build

Cross build for armv7-unknown-linux-gnueabihf (default target for cross).

```sh
cross build
```

## How to contribute

This projects uses the *feature* branches for ongoing development. Use a pull
request (PR) to merge your changes back to *main*.

Prior to your PR please install and use pre-commit hooks.

```console
# Install pre-commit.
$ pip install pre-commit

# Install hooks.
$ pre-commit install
pre-commit installed at .git/hooks/pre-commit
```

The following command allows to bypass pre-commit verifications:

```sh
git commit -m 'all: quick fix' --no-verify
```

See <https://pre-commit.com/#installation> for more information.
