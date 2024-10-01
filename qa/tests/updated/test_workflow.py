# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: <text>
# Copyright(c) 2026 Liebherr-Digital Development Center GmbH
# Written by Thomas Witte <thomas.witte@liebherr.com>
# </text>

import os
import shutil
import time


def test_example_workflow(cargo):
    print("Running example workflow test...")

    # create the update
    shutil.rmtree("/tmp/example_workflow")
    os.makedirs("/tmp/example_workflow/source", exist_ok=True)
    with open("/tmp/example_workflow/source/hello.txt", "w") as f:
        f.write("Hello, World!")
    with open("/tmp/example_workflow/source/update.json", "w") as f:
        f.write('{"update_id": "1234", "metadata": { }, "files": ["hello.txt"]}')

    # wait for the update to install
    time.sleep(3)
    cargo.terminate()
    cargo.wait(1)

    print("Evaluating output...")
    lines = list(cargo)
    update_done = [line for line in lines if "Update completed successfully." in line]
    assert update_done
    with open("/tmp/example_workflow/target/hello.txt", "r") as f:
        assert f.read() == "Hello, World!"
