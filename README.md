# Noodle

Stream parsing tools for Rust.

## What You Can Do

- `ReadMuncher`: Continuously grab bytes from `Read` objects, and iterate over byte packets/parcels.
  - Huge binary files (where loading the entire thing into memory is a bad idea).
  - I/O streams, like TCP or serial ports.

## Future Plans

- Support `AsyncRead` and `Stream`.
- Make generic over anything implementing `IntoIterator<u8>`.
- Make it work better with nom v5.0, by parsing `InputTake` instead of just `&[u8]` (`str` included).
