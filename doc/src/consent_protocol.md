# MQTT User Consent / Update Protocol specification

The user consent protocol uses MQTT messages with JSON encoded payload.
Four message types are currently defined:

* ConsentRequestMsg: user consent to install an update is requested by
  *updated*.
* ConsentResponseMsg: consent is given or declined by the *UI*.
* CheckForUpdatesMsg: the *UI* requests polling for pending updates.
* UpdateProgressMsg: *updated* sends feedback on the current update progress.

Each message type uses a respective MQTT topic:

| Message Type | Topic Name | Sender -> Recv |
|--------------|------------|----------------|
|ConsentRequestMsg|updated/v1/ConsentRequest|updated -> UI|
|ConsentResponseMsg|updated/v1/ConsentResponse|UI -> updated|
|CheckForUpdatesMsg|updated/v1/CheckForUpdates|UI -> updated|
|UpdateProgressMsg|updated/v1/UpdateProgress|updated -> UI|

## ConsentRequestMsg

A request for consent always contains an internal, sequential id of the update
deployment, and a consent token which must be present and equal in the
response.

Additionally, a consent request can contain metadata, i.e. a map of arbitrary
keys and string values. The metadata can be freely chosen with each update, but
should contain:

* a *version*: a string identifier of the installed update that can be
  displayed in the UI, e.g. "1.1.0".
* a *changelog*: a language independent string representation of the changes in
  the update, e.g. keywords, a bitmask, … This representation should be
  interpreted by the UI and displayed according to the current locale.

A request for consent has an internal timeout, after which the *consent_token*
becomes invalid and a new ConsentRequestMsg is sent.

> Example: Request for consent

```json
{
    "update_id":"10",
    "metadata":{
        "version":"1.1.0",
        "changelog":"functional|security|diagnostics",
    },
    "consent_token":13762873
}
```

## ConsentResponseMsg

The response to a request for consent contains the *consent_token* of the
request and the *consent* status, which is either `"accepted"` or `"declined"`.

> Example: Consent given

```json
{"consent":"accepted","consent_token":5167242}
```

> Example: Consent declined

```json
{"consent":"declined","consent_token":463812}
```

## CheckForUpdatesMsg

A CheckForUpdatesMsg is an empty message that triggers *updated* to contact the
server and poll for pending updates. If an update is pending and needs the user
to consent, *updated* sends a ConsentRequestMsg.

If a pending update was previously declined, it will be shown again.

> Example: Poll for Updates

```json
{}
```

## UpdateProgressMsg

During an update, *updated* will give feedback on its progress. An
UpdateProgressMsg contains the internal *update_id*, a
*cnt*/*of* (u32) pair to measure progress (e.g. step 3/5, or 0/0
if no progress is available) and a current state which might be one of the
following:

* `"pending"`: The update will start shortly
* `"downloading"`: The update is currently downloading
* `"installing"`: The update is currently installing
* `"finished"`: The update finished successfully
* `"{"failed":"<reason>"}"`: The update failed with an error code `<reason>`

The update might not use all states but *updated* will at least inform the UI
about *finished* and *failed* updates.

In case the *update_with_consent_flow* workflow is used, updated will use the
following progress indications for different steps:

* (0/5) consent given, update started
* (downloaded_size/update_size) in B, KiB, MiB… depending on the file size
  while downloading
* (2/5) installing
* (3/5) awaiting reboot
* (4/5) self-test
* (5/5) finished / failed

> Example: Update installing

```json
{
    "update_id":"4",
    "cnt":2,
    "of":5,
    "state":"installing"
}
```

> Example: Update failed

```json
{
    "update_id":"4",
    "cnt":5,
    "of":5,
    "state":{"failed":"error installing update"}
}
```
