#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: <text>
# Copyright(c) 2026 Liebherr-Digital Development Center GmbH
# Written by Thomas Witte <thomas.witte@liebherr.com>
# </text>

from pathlib import Path

import pytest
import sh
import json
import os


def pytest_addoption(parser):
    parser.addoption(
        "--cross",
        action="store_true",
        help="Build with cross toolchain",
    )
    parser.addoption(
        "--profile",
        action="store",
        default="release",
        help="Profile (release, dev, ...)",
    )
    parser.addoption(
        "--target",
        action="store",
        default="x86_64-unknown-linux-gnu",
        choices=[
            "x86_64-unknown-linux-gnu",
            "armv7-unknown-linux-gnueabihf",
            "x86_64-pc-windows-gnu",
        ],
        help="Platform target",
    )


def pytest_collection_modifyitems(config, items):
    cross = config.getoption("--cross")
    if not cross:
        return
    skip_cross = pytest.mark.skip(reason="not supported with cross toolchain")
    for item in items:
        if "no_cross" in item.keywords:
            item.add_marker(skip_cross)


@pytest.fixture()
def bin_env():
    """Environment variables for cargo invokation."""
    os.environ["NO_COLOR"] = "1"
    return os.environ


@pytest.fixture()
def cargo(bin_env, request):
    """Provides a running instance of the cargo binary."""
    cmd = sh.Command("cross" if request.config.getoption("--cross") else "cargo")
    metadata = cmd(["metadata", "--no-deps", "--format-version", "1"])
    packages = json.loads(str(metadata))
    names = [package["name"] for package in packages["packages"]]
    assert Path(request.path).parent.name in names
    bin = cmd(
        [
            "run",
            "--bin",
            Path(request.path).parent.name,
            "--profile",
            request.config.getoption("--profile"),
            "--target",
            request.config.getoption("--target"),
            "--",
            "-w",
            "example_workflow",
            "-c",
            "qa/conf/updated.sample.env",
        ],
        _iter=True,
        _env=bin_env,
    )
    yield bin
    if bin.is_alive():
        bin.kill()
