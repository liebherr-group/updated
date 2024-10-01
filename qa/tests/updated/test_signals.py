# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: <text>
# Copyright(c) 2026 Liebherr-Digital Development Center GmbH
# Written by Thomas Witte <thomas.witte@liebherr.com>
# </text>

from signal import Signals

import pytest
import sh


_linux_sigs = [Signals.SIGHUP, Signals.SIGINT, Signals.SIGQUIT, Signals.SIGTERM]


def pytest_generate_tests(metafunc):
    if "signal" in metafunc.fixturenames:
        signals = _linux_sigs
        metafunc.parametrize("signal", signals)


def test_signal_handlers(cargo, signal):
    """Test if raised signals lead to clean shutdown."""
    # Send signals and wait for clean exit code.
    with pytest.raises(sh.TimeoutException):
        cargo.wait(timeout=3)
    cargo.signal(signal)
    cargo.wait(timeout=1)
    assert cargo.exit_code == 0
    # Read lines from stdout and ensure the signal caused the exit.
    lines = list(cargo)
    shutdown_found = [line for line in lines if "Shutting down." in line]
    assert shutdown_found
    signal_found = [
        line for line in lines if f'Signal received. signal="{signal.name}"' in line
    ]
    assert signal_found
