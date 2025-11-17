# Anchor

Anchor is an implementation of the Klipper protocol.

You can use Anchor to create custom Klipper MCUs. It's written in Rust and
provides only the protocol implementation, giving you full control over how you
want to tie the protocol handling in to your program. You can even implement a
Klipper MCU that runs over a PTY, like the Linux `klipper_mcu` program.

This repo contains the following folders:

  * `anchor`  
    The runtime support library for Anchor. It includes all the functionality
    that is used during runtime.

  * `anchor_codegen`  
    The Klipper protocol requires exchanging an initial data dictionary, and
    command IDs need to be hooked up. `anchor_codegen` creates this data
    dictionary, message handlers, serializers, etc. and generates a Rust module
    that will be included by the `klipper_generate_config` macro in your
    project.

  * `rp2040_demo`  
    A simple demo showing how one could integrate Anchor in an rp2040 project,
    communicating over USB.

  * `esp32c3_demo`  
    A simple demo showing how one could integrate Anchor in an esp32c3 project,
    communicating over USB.

  * `testjig`  
    A development tool and example of how to use Anchor for implementing a very
    simple PTY based MCU that can talk to Klipper.

  * `anchor_macro`  
    Implements the `proc_macro`s needed by `anchor`. You shouldn't have to mess
    with this.

  * `anchor_types`  
    Used internally to provide the `KlipperCommandFlags` struct which `anchor` re-exports.

Anchor powers the [Beacon3D Surface Scanner](https://beacon3d.com/).

## Features

### Error Handling

Commands can return errors using Rust's `Result` type:

```rust
#[derive(Debug)]
pub enum ConfigError {
    NotFound,
}

#[klipper_command]
fn get_config() -> Result<(), ConfigError> {
    // ... implementation ...
    if config_not_found {
        Err(ConfigError::NotFound)
    } else {
        Ok(())
    }
}
```

Errors are automatically wrapped in a generated `KlipperCommandError` enum and can be handled in your main loop when calling `Transport::receive()`.

### Command Flags

Commands can be marked with flags to control their behavior. Currently supported flags:

- `HF_IN_SHUTDOWN`: Allows the command to execute during shutdown state

```rust
use anchor::KlipperCommandFlags;

#[klipper_command(flags = KlipperCommandFlags::HF_IN_SHUTDOWN)]
fn clear_shutdown() {
    // This command can run even when system is in shutdown state
}
```

Commands without this flag will be ignored during shutdown, allowing only recovery commands to execute.

### Shutdown State Filtering

When the `shutdown-filtering` feature is enabled, Anchor can automatically filter commands based on shutdown state. This requires:

1. Enabling the feature in both `anchor` and `anchor_codegen`:
   ```toml
   [dependencies]
   anchor = { path = "../anchor", features = ["shutdown-filtering"] }
   
   [build-dependencies]
   anchor_codegen = { path = "../anchor_codegen", features = ["shutdown-filtering"] }
   ```

2. Implementing `CheckShutdown` on your context type:
   ```rust
   use anchor::CheckShutdown;
   
   struct MyContext<'a> {
       is_shutdown: &'a bool,
   }
   
   impl<'a> CheckShutdown for MyContext<'a> {
       fn is_shutdown(&self) -> bool {
           *self.is_shutdown
       }
   }
   ```

3. Passing the context when calling `receive()`:
   ```rust
   let mut is_shutdown = false;
   let context = MyContext { is_shutdown: &is_shutdown };
   transport.receive(&mut buffer, context)?;
   ```

Commands marked with `HF_IN_SHUTDOWN` will be allowed to execute during shutdown, while others will be automatically filtered out.

## Documentation

Documentation can be found [here](https://anchor.annex.engineering).

## Licensing

Anchor is licensed under the MIT license, which means you can do pretty much
whatever you want with it. Please see [LICENSE.txt](LICENSE.txt) for more
information.

## Anchor Projects

We want to showcase new and interesting uses of Anchor as we are very excited
about this new implementation. If you've made something with Anchor, drop a
line. We'd love to add you to this list.

  * [Beacon3D](https://beacon3d.com/) is a novel eddy current surface scanner
    that can scan your bed in seconds.

  * [Crampon](https://github.com/Annex-Engineering/crampon_anchor) This nozzle
    based accelerometer was reimplemented in Anchor to demonstrate how quickly
    one can bring up a new device.

  * [Rampon](https://github.com/rogerlz/rampon_anchor) The popular KUSBA
    resonance measuring device firmware was reimplemented using Anchor as well,
    fixing some minor firmware bugs in the original implementation.

## Acknowledgements

This project is in no way endorsed by the Klipper project. Please do not direct
any support requests to the Klipper project.

  * [Klipper](https://www.klipper3d.org/) by [Kevin O'Connor](https://www.patreon.com/koconnor)
