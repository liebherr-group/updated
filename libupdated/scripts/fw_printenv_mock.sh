#!/bin/bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: <text>
# Copyright(c) 2026 Liebherr-Digital Development Center GmbH
# Written by Thomas Witte <thomas.witte@liebherr.com>
# </text>

firmware_file="/tmp/uboot_env.txt"
touch $firmware_file

# get the number of arguments to this script
num_args=$#

# if the number of arguments is 0, print the whole environment
if [ $num_args -eq 0 ]; then
    cat $firmware_file
    exit 0
fi

# if the number of arguments is 1, this key should be read from the "firmware" file
if [ $num_args -eq 1 ]; then
    key=$1

    # if the key is "clear", delete the firmware file
    if [ "$key" == "clear" ]; then
        rm $firmware_file
        exit 0
    fi

    # read the key from the "firmware" file
    value=$(cat $firmware_file | grep "$key=" | cut -d'=' -f2)
    echo "$key=$value"
    exit 0
fi

exit 1
