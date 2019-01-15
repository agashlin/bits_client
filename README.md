update_agent
============

Currently this is only code for interfacing with BITS.


bits_client lib
---------------

`bits_client` is the primary target and provides `BitsClient`, an API for creating and monitoring BITS jobs.

`bits_client::new()` creates a `BitsClient` that does all operations within the current process, as the current user.

test_client example
-------------------

`examples/test_client.rs` shows how to use the API.
