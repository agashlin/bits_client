update_agent
============

Currently this is only code for interfacing with BITS.


bits_client lib
---------------

`bits_client` is the primary target and provides `BitsClient`, an API for creating and monitoring BITS jobs.

`bits_client::new()` creates a `BitsClient` that does all operations within the current process, as the current user.

If built with the "external_task" feature, you can use `bits_client::connect_task()` to get a `BitsClient` that will create and access jobs as Local Service, provided the `update_agent` bin below has been installed as a scheduled task.


update_agent bin
----------------

`update_agent` is a bin in `src/main.rs`, this is meant to run as a scheduled task, it currently has no triggers and so is on-demand only. It runs as Local Service in order to provide cross-system access to BITS jobs.

test_client example
-------------------

`examples/test_client.rs` shows how to use the API. When built with the "external_task" feature it will use the scheduled task, otherwise it will operate in-process.
