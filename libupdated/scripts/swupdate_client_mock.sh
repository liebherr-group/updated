#!/bin/sh
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: <text>
# Copyright(c) 2026 Liebherr-Digital Development Center GmbH
# Written by Thomas Witte <thomas.witte@liebherr.com>
# </text>

REPORT_FILE="/tmp/$(basename "${0}").txt"

# add the command line arguments to the report file
echo "cmdline: \"$*\"" > "$REPORT_FILE"

# send feedback to the user
echo "Status: 1 message: installing update"

# add the stdin to the report file
echo "stdin: \"$(cat)\"" >> "$REPORT_FILE"

echo "Status: 0 message: update installed"
