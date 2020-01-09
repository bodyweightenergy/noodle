# Noodle

For when you need to parse incomplete I/O streams, where not all bytes are available at once (reading huge files, network streams, other I/O byte streams).

## What You Can Do

- `ReadMuncher`: Continuously grab bytes from `Read` objects, and iterate over byte packets/parcels.

## Future Plans

- Support `AsyncRead` and `Stream`.
- Make generic over anything implementing `IntoIterator<u8>`.
- Add optional adaptors for nom v5.0, by parsing `InputTake` instead of just `&[u8]` (`str` included).
