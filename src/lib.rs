/*!

Provides byte stream parsing utilities.

```rust,ignore

let reader = File::open("..."); // reader is anything that implements io::Read
let alloc_size = 1000;  // Set custom allocation size
let muncher = ReadMuncher::<DataItem, _>::new(&reader, alloc_size, |bytes, is_eof| {
    // parse function here
});

for packet in &muncher {
     // packet is DataItem
}

```
*/

use anyhow::Error;
use std::io::Read;
use thiserror;

pub type MunchOutput<T> = Option<(T, usize)>;

/// Error types for this library
#[derive(Debug, thiserror::Error)]
pub enum MunchError {
    /// Error while reading
    #[error("Error reading from Read object")]
    Read(#[from] std::io::Error),

    #[error("Unknown")]
    Unknown,
}

/** Continuously reads bytes from a `Read` implementor,
 parses that byte stream with the provided parser function,
 and provides an iterator over the parsed slices/packets/parcels.

```rust
# use noodle::ReadMuncher;
# fn main() {
use std::io::Cursor;

let mut read = Cursor::new(vec![0xff, 1, 0xff, 2, 2, 0xff, 3, 3, 3]);

let munched: Vec<Vec<u8>> = ReadMuncher::new(&mut read, 5, |b, _| {
    if b.len() >= 2 {
        let skip = b[1] as usize + 1;
        if b.len() > skip {
            let blob = &b[..skip];
            Ok(Some((blob.to_owned(), skip)))
        } else {
            Ok(None)
        }
    } else {
        Ok(None)
    }
})
.collect();

assert_eq!(munched.len(), 3);
assert_eq!(munched[0], vec![0xff, 1]);
assert_eq!(munched[1], vec![0xff, 2, 2]);
assert_eq!(munched[2], vec![0xff, 3, 3, 3]);

 # }
 ```
*/
pub struct ReadMuncher<'a, T, F>
where
    F: Fn(&[u8], bool) -> Result<MunchOutput<T>, Error>,
{
    reader: &'a mut dyn Read,
    buffer: Vec<u8>,
    parse_location: usize,
    alloc_size: usize,
    // parse_fn: &'b ParseFn<T>,
    parse_fn: F,
    complete: bool,
    read_end_location: usize,
}

impl<'a, T, F> ReadMuncher<'a, T, F>
where
    F: Fn(&[u8], bool) -> Result<MunchOutput<T>, Error>,
{
    /// Starts a new `ReadMuncher` instance.
    ///
    /// ## reader
    ///
    /// The `Read` implementor to be read for bytes.
    ///
    /// ## alloc_size
    ///
    /// How many more bytes to allocate at end of the read buffer when existing bytes not sufficient for parsing.
    /// Currently, there's no limit to how big the buffer can get.
    /// Also, sets initial size of read buffer.
    ///
    /// ## parse_fn
    ///
    /// The parse function invoked over the read buffer.
    ///
    pub fn new(reader: &'a mut dyn Read, alloc_size: usize, parse_fn: F) -> Self {
        Self {
            reader,
            alloc_size,
            buffer: Vec::with_capacity(alloc_size),
            parse_location: 0,
            parse_fn,
            complete: false,
            read_end_location: 0,
        }
    }

    /// Removes used bytes from buffer, and shifts everything to start.
    fn resize_no_alloc(&mut self) -> Result<Option<Vec<u8>>, Error> {
        let full_len = self.buffer.len();
        self.buffer.drain(0..self.parse_location);
        self.buffer.resize_with(full_len, || 0);
        self.read_end_location -= self.parse_location;
        let num_new_bytes = self
            .reader
            .read(&mut self.buffer[(full_len - self.parse_location)..])?;
        self.read_end_location += num_new_bytes;
        if num_new_bytes == 0 {
            let blob = &self.buffer[..self.read_end_location];
            self.complete = true;
            Ok(Some(blob.to_owned()))
        } else {
            self.parse_location = 0;
            Ok(None)
        }
    }

    /// Extends the buffer to allow storing more bytes.
    fn resize_alloc(&mut self) -> Result<Option<Vec<u8>>, Error> {
        let old_len = self.buffer.len();
        self.buffer.resize_with(old_len + self.alloc_size, || 0);
        let num_new_bytes = self.reader.read(&mut self.buffer[old_len..])?;
        self.read_end_location += num_new_bytes;
        if num_new_bytes == 0 {
            let blob = &self.buffer[..self.read_end_location];
            self.complete = true;
            Ok(Some(blob.to_owned()))
        } else {
            Ok(None)
        }
    }
}

impl<'a, T, F> Iterator for ReadMuncher<'a, T, F>
where
    F: Fn(&[u8], bool) -> Result<MunchOutput<T>, Error>,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.complete == true {
            return None;
        }
        loop {
            // let buf: &'b [u8] = &self.buffer[self.parse_location..];
            let parse_result = (self.parse_fn)(&self.buffer[self.parse_location..], false);
            match parse_result {
                Ok(r) => match r {
                    // Parse complete
                    Some((item, n)) => {
                        self.parse_location += n;
                        return Some(item);
                    }
                    // Parse incomplete
                    None => {
                        if self.parse_location != 0 {
                            match self.resize_no_alloc() {
                                Ok(r) => {
                                    if let Some(last) = r {
                                        match (self.parse_fn)(&last, true) {
                                            Ok(Some((item, _))) => return Some(item),
                                            _ => return None,
                                        }
                                    }
                                }
                                Err(_) => return None,
                            }
                        } else {
                            match self.resize_alloc() {
                                Ok(r) => {
                                    if let Some(last) = r {
                                        match (self.parse_fn)(&last, true) {
                                            Ok(Some((item, _))) => return Some(item),
                                            _ => return None,
                                        }
                                    }
                                }
                                Err(_) => return None,
                            }
                        }
                    }
                },
                Err(e) => {
                    eprintln!("Malformed input. {:?}", e);
                    return None;
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// Tests passing static function as parser.
    #[test]
    pub fn static_fn() {
        use std::io::Cursor;
        use std::result::Result;

        fn my_parse_function(bytes: &[u8], _: bool) -> Result<MunchOutput<Vec<u8>>, Error> {
            if bytes.len() >= 2 {
                let skip = bytes[1] as usize + 1;
                if bytes.len() > skip {
                    let blob = &bytes[..skip];
                    Ok(Some((blob.to_owned(), skip)))
                } else {
                    Ok(None)
                }
            } else {
                Ok(None)
            }
        }

        let mut read = Cursor::new(vec![0xff, 1, 0xff, 2, 2, 0xff, 3, 3, 3]);

        let munched: Vec<Vec<u8>> = ReadMuncher::new(&mut read, 5, my_parse_function).collect();

        assert_eq!(munched.len(), 3);
        assert_eq!(munched[0], vec![0xff, 1]);
        assert_eq!(munched[1], vec![0xff, 2, 2]);
        assert_eq!(munched[2], vec![0xff, 3, 3, 3]);
    }

    /// Tests passing closure as parser function.
    #[test]
    pub fn closure() {
        use std::io::Cursor;

        let mut read = Cursor::new(vec![0xff, 1, 0xff, 2, 2, 0xff, 3, 3, 3]);

        let munched: Vec<Vec<u8>> = ReadMuncher::new(&mut read, 5, |b, _| {
            if b.len() >= 2 {
                let skip = b[1] as usize + 1;
                if b.len() > skip {
                    let blob = &b[..skip];
                    Ok(Some((blob.to_owned(), skip)))
                } else {
                    Ok(None)
                }
            } else {
                Ok(None)
            }
        })
        .collect();

        assert_eq!(munched.len(), 3);
        assert_eq!(munched[0], vec![0xff, 1]);
        assert_eq!(munched[1], vec![0xff, 2, 2]);
        assert_eq!(munched[2], vec![0xff, 3, 3, 3]);
    }
}
