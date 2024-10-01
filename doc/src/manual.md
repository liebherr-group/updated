# Manual

## Updated cli interface

``` text
Usage: updated --workflow <WORKFLOW> --config <CONFIG>

Options:
  -w, --workflow <WORKFLOW>
  -c, --config <CONFIG>
  -h, --help                 Print help
  -V, --version              Print version
```

### --workflow

Select the workflow that should be started by name. Updated prints all
available workflows if started with an invalid workflow. An example workflow
`example_workflow` exists.
More information how to create your own workflows can be found [here](#defining-custom-update-workflows).

### --config

Select the configuration file for the selected workflow. The config is a
key-value text file in either .env or .json format (determined by file
extension).

If the .env format is used, missing keys are searched in updated's environment
variables (but keys in the config file always take precedence). For .env files,
keys (not values!) are case insensitive to better match bash naming
conventions.

The configuration keys are workflow specific and loaded/validated by the
workflow itself.

## Workflow configuration

Depending on the workflow numerous configuration keys to configure its
hawkbit/mqtt/… connection are required. Unless noted otherwise, all
configuration keys are mandatory and must be set for *updated* to start.

### Hawkbit configuration

#### sota_hawkbit_url

The base URL of the hawkbit server, e.g. `http:\\localhost:8080` in case of the
hawkbit docker container.

#### sota_tenant

The tenant on the hawkbit server. This should be set to `default` in most cases.

#### sota_target_id

The client's target ID to identify itself with the hawkbit server. This should
be a unique id per client, e.g. a machine ID. On the hawkbit server, it might
be necessary to manually create a target with this ID.

#### sota_auth_mode

**(gateway|target|none)**
The authentification mode with the hawkbit server. Possible values are
`gateway`, `target`, or `none` (case insensitive). If an
authorization via a gateway or target token is required, `sota_auth_token` must
be set aswell.

#### sota_auth_token

The token that is sent to authenticate the client with the server.

#### sota_default_polling_interval_sec

**(u64)**
The default polling interval the client uses. After successfully connecting to
hawkbit, the server normally proposes its own polling interval, which takes
precedence.

#### sota_server_cert

**optional**
The certificate file (pem) that should be used to authenticate the hawkbit
server. This parameter is necessary, if the server uses, e.g., a self-signed
TLS certificate.

#### sota_client_cert

**optional**
The client certificate (.pem file) used to authenticate the client with the
server.

#### sota_attributes_script

**optional**
Generate the response to a config request from Hawkbit from a script instead of
sending default information. The output of the script should be valid json, e.g.:

``` json
{
  "PRODUCT_VERSION": "1.0.2-32-g80ac9d1-dirty",
  "PRODUCT_NAME": "liebherr-linux-platform",
  "OS_ID": "liebherr",
  "OS_VERSION_ID": "2024.06-9-g9131a13"
}
```

### Persistent storage configuration

#### sota_state_file

The json file in which the state of consent requests is persisted. The version
number of any update that is declined will be added as *declined* and the
update will be ignored if `UpdateOptions::ignore_previously_declined` is set
(*true* for automatic polling, *false* for manual search for updates).

### MQTT configuration

#### mqtt_consent_host

The hostname on which the mqtt broker runs.

#### mqtt_consent_port

**(u16)** The port on which the mqtt broker runs.

#### mqtt_consent_request_topic

The mqtt topic name updated sends consent request messages to if a pending
update needs consent.

#### mqtt_consent_response_topic

The name of the topic on which a response to a consent request (i.e. the user
accepts or declines it) is received.

#### mqtt_manual_update_topic

The name of the topic on which manual update triggers (i.e. a user clicks check
for updates now) are received.

#### mqtt_update_progress_topic

The name of the topic on which update progress is published.

### SWUpdate configuration

#### swupdate_use_streaming

**(true|false)** Directly stream the update to the target partition without
downloading it first. This might be necesssary if the update file is large but
can be dangerous without a rollback possibility if the connection to the update
source is unstable.

#### swupdate_run_postupdate

**(true|false)** Run swupdate postupdate actions after a successful update.

#### swupdate_dry_run

**(true|false)** Do not actually install the update but only simulate it.

#### swupdate_tmp_dir

The directory, the swu file is downloaded to if streaming is not used. Make
sure that there is enough free space available to fit the swu file.

#### swupdate_bufsize

**(usize)** Blocksize used to read the downloaded swu file is streaming is not
used. A good value could be ~1MB (1048576) if RAM is not severely limited.

#### swupdate_client_bin

The swupdate client binary that should be used to install the update. Probably
`/usr/bin/swupdate-client`.

#### swupdate_ipc_socket

The ipc socket that should be used to communicate with the swupdate daemon.
Probably `/run/swupdate/sockinstctrl`.

#### swupdate_software_set

**optional** The software set key in the `sw-description` that is used to
determine how the update is installed.

This key is optional; if not set, the settings of the swupdate daemon are used.
Note, that the swupdate validates this setting against a list of configurations
(swupdates -q command line flag).

This setting must be used together with the [swupdate_running_mode](#swupdate_running_mode)
setting.

#### swupdate_running_mode

**optional** The running mode key in the `sw-description` that is used to
determine how the update is installed. In case of an A/B updateable distro,
this should be set to `root_a` or `root_b`, depending on the target partition
that the update is installed to.

This key is optional; if not set, the settings of the swupdate daemon are used.
Note, that the swupdate validates this setting against a list of configurations
(swupdates -q command line flag).

This setting must be used together with the [swupdate_software_set](#swupdate_software_set)
setting.

#### swupdate_setenv_bin

Path to the binary that should be called to set variables in the U-Boot
environment. Typically, this should be set to `/usr/bin/fw_setenv`.

#### swupdate_printenv_bin

Path to the binary that should be called to read variables in the U-Boot
environment. Typically, this should be set to `/usr/bin/fw_printenv`.

## Defining custom update workflows

You can define and add your own update workflow to updated by creating a
function with the following signature and `#[workflow]` annotation:

```rust
use libupdated::workflow;

#[workflow]
fn my_workflow(config: HashMap<String, String>) -> Result<(), WorkflowError> {
    …
}
```

If the workflow is exiting updated with an exit code, this should be annotated
to let updated handle terminating the process:

```rust
use libupdated::workflow;
use std::process::ExitCode;

#[workflow(exiting)]
fn my_exiting_workflow(config: HashMap<String, String>)
    -> Result<ExitCode, WorkflowError>
{
    …
    Ok(ExitCode::from(42))
}
```

Make sure the package that defines your workflow is linked against updated. If
you define your workflow in another crate, it might be necessary to add
`extern crate <my_workflow_lib>;` to updated's `main.rs` to force linking your
library.

Your workflow can be selected by its name when starting updated:

``` sh
updated --workflow my_workflow --config my_workflow_config.json
```

## Using updated as a systemd service

Updated supports systemd's notify and watchdog features. It sends a notify
message at startup to signal it is running (unit type notify). If the presence
of a systemd watchdog timer is detected, a keep alive signal is automatically
sent with double the timeout frequency, i.e. every 2s if the timeout is 4s.
