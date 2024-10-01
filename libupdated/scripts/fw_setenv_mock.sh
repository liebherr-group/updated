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

# if the number of arguments is 1, this key should be removed from the "firmware" file
if [ $num_args -eq 1 ]; then
    key=$1

    # if the key is "clear", delete the firmware file
    if [ "$key" == "clear" ]; then
        rm $firmware_file
        exit 0
    fi

    # remove the key from the "firmware" file
    sed -i "/$key=/d" $firmware_file
    exit 0
fi

# if the number of arguments is 2, this key should be written to the "firmware" file
if [ $num_args -eq 2 ]; then
    key=$1
    value=$2

    # if the key already exists in the file, replace it
    if grep -q "$key=" $firmware_file; then
        sed -i "s/$key=.*/$key=$value/g" $firmware_file
        exit 0
    else
        # if the key does not exist in the file, append it
        echo "$key=$value" >> $firmware_file
        exit 0
    fi
fi

exit 1
