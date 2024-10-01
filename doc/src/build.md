# Building Updated

Updated is built using cargo. It is tested on both Linux on x86-64 and armv7
platforms.

## Quickstart Guide

The updated application including a simple example workflow can be build with
`cargo build --bin updated`.

The example workflow periodically checks a *source* directory for updates and
copies these update files to a *target* directory. On the next restart, the
system integrity is checked by executing `systemctl is-system-running` in order
to ensure the update did not degrade the system.

The example workflow can be executed by running

``` sh
cargo run --bin updated -- \
--workflow example_workflow \
--config qa/conf/updated.sample.env
```

The example configuration must set the interval for checking for updates, the
source and target directories, as well as the file for persisting updated's
state.

Running updated will produce output to the following and can be stopped with
Ctrl-C:

``` log
 INFO libupdated: Systemd watchdog is not enabled.
 INFO updated: Available workflow: example_workflow
 INFO libupdated: Updated SOTA Updater. version="606764f-dirty"
      date="2025-06-17" target="x86_64-unknown-linux-gnu"
 INFO example_workflow: Workflow configuration finished, starting update loop.
      Configuration: { [...] }
 INFO libupdated::file_store: Creating a empty store file /tmp/example_workflow/state.json,
      as it does not exist yet LOG="libupdated"
 INFO libupdated: Slept for 10 seconds. Time to check for updates.
ERROR example_workflow: Update result: No pending updates.
 INFO libupdated: Slept for 10 seconds. Time to check for updates.
ERROR example_workflow: Update result: No pending updates.
...
^C
 INFO libupdated: Signal received. signal="SIGINT"
 INFO libupdated: Shutting down.
```

To schedule an update, create the directory `/tmp/example_workflow/source` and
place one or more files to be copied (the update, in this case `update.zip`) as
well as an update manifest in the source directory:

> Update manifest update.json

``` json
{"update_id": "1", "metadata": {}, "files": ["update.zip"]}
```

Now the update should be picked up and copied to the target folder the next
time the update is triggered.

``` log
 INFO libupdated: Slept for 10 seconds. Time to check for updates.
 INFO libupdated::log_reporter: Update progress (1): Pending (0/5) - Update
      consent given, starting update
 INFO libupdated::log_reporter: Update progress (1): Installing (1/1) - Status:
      success Message: installed update to /tmp/example_workflow/target
 INFO libupdated::log_reporter: Update progress (1): Pending (3/5) - Update
      installed, checking success after next reboot
 INFO example_workflow: Update completed successfully.
```

And after restarting updated, the system integrity is checked (failing in this
example log):

``` log
 INFO libupdated::log_reporter: Update progress (1): Pending (4/5) - Update
      installed, testing in progress
 INFO libupdated::log_reporter: Update progress (1): Failed("Rollback needed:
      systemctl is-system-running failed with 'degraded\n'") (2/2)
      - Post-update check failed: Rollback needed: systemctl is-system-running
      failed with 'degraded'
ERROR example_workflow: Previous update failed: Rollback needed: systemctl
      is-system-running failed with 'degraded'
```

## Cargo Features

The cargo build of *libupdated* offers some additional optional features,
listed below:

* **hawkbit:** builds the hawkbit update source and progress reporter to fetch
  updates from a hawkbit server and report progress using its DDI API.
* **mqtt:** builds the MQTT module to send and receive user consent messages,
  and send progress messages via MQTT.
* **swupdate:** builds the SWUpdate installer integration to connect to a
  swupdate service and feed it a *swu* file.
* **uboot:** support for persisting updated state in the U-Boot environment and
  checking the U-Boot environment to retrieve the swupdate status.
* **systemd:** support for feeding the systemd watchdog if updated is started
  as a systemd service with the *WatchdogSec* service option.
* **cli:** support for a simple CLI interface to set the workflow and
  configuration through command line flags (`--workflow`, `--config`)

## Running Tests

Testing for updated is done with cargo and pytest. The cargo testsuite can be
executed by running `cargo test --all --all-features`.

The pytest testsuite can be executed by first installing the required
dependencies in a venv, then executing pytest in that environment:

``` sh
pip install .
source .venv/bin/activate
pytest --profile=release -vv -s
```

## Test Coverage

``` sh
# install coverage tool
cargo install grcov
rustup component add llvm-tools-preview
# instrumented test run
RUSTFLAGS="-C instrument-coverage" LLVM_PROFILE_FILE="updated-%p-%m.profraw" \
cargo test --all --all-features
# create report
grcov . --binary-path ./target/debug/ -s . -t html --branch \
--ignore-not-existing --ignore "*target*" -o coverage_report
```

## Build Documentation

The documentation is built using mdbook and rustdoc:

``` sh
cargo install mdbook
mdbook build
cargo doc --no-deps
```

## License Overview

The licenses of all dependencies can be checked for compatibility to a set of
allowed licenses and a summary of all license texts can be generated using
*cargo-about*:

``` sh
cargo install cargo-about
cargo about generate -o licenses.html about.hbs
```
