/*!

Provides synchronous byte stream parsing utilities.

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

/// Output type for `ReadMuncher` parse function
/// 
/// `T` is the iterator output type (e.g. `Packet` or `Frame` structs).
/// 
/// `usize` is the number of bytes consumed to created the output type.
pub type MunchOutput<T> = Option<(T, usize)>;

/** Continuously reads bytes from a `Read` implementor,
 parses that byte stream with the provided parser function,
 and provides an iterator over the parsed slices/packets/frames.

```rust
# use noodle::ReadMuncher;

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

 ```
*/
pub struct ByteMuncher<T, F>
where
    F: Fn(&[u8], bool) -> Result<MunchOutput<T>, Error>,
{
    source: Box<dyn Iterator<Item = u8>>,
    buffer: Vec<u8>,
    parse_location: usize,
    alloc_size: usize,
    parse_fn: F,
    complete: bool,
    read_end_location: usize,
}

impl<T, F> ByteMuncher<T, F>
where
    F: Fn(&[u8], bool) -> Result<MunchOutput<T>, Error>,
{
    /// Creates a new `ByteMuncher` instance from a boxed byte iterator.
    ///
    /// ### source
    ///
    /// The boxed `Iterator<Item = u8>` object used a bytes source.
    ///
    /// ### alloc_size
    ///
    /// Internal buffer allocation increment size.
    ///
    /// This sets the initial buffer size, and size increase increments when necessary.
    ///
    /// ### parse_fn
    ///
    /// The parse function called against the read buffer. This can be a static function or a closure, with signature `Fn(&[u8], bool) -> Result<MunchOutput<T>, Error>`.
    ///
    /// The first parameter is a reference to the unconsumed slice of the read buffer.
    /// The second parameter is a boolean, which signals EOF (no more bytes available to read).
    ///
    /// The return type is used internally to consume/step forward. Below is some example return values and what they do:
    ///
    /// ```ignore
    ///
    /// Ok(Some((item, 12)))  // 'item' is returned, buffer drains 12 bytes
    ///
    /// Ok(None)  // Not enough bytes, keep trying
    ///
    /// Err(...)  // Error occurred. Iterator panics and prints to stderr
    ///
    /// ```
    pub fn new(source: Box<dyn Iterator<Item = u8>>, alloc_size: usize, parse_fn: F) -> Self 
     {
        Self {
            source,
            alloc_size,
            buffer: Vec::with_capacity(alloc_size),
            parse_location: 0,
            parse_fn,
            complete: false,
            read_end_location: 0,
        }
    }

    /// Creates a new `ByteMuncher` from a boxed `Read` object.
    pub fn from_read(source: Box<dyn Read>, alloc_size: usize, parse_fn: F) -> Self {
        Self::new(
            Box::new(source.bytes().map(|r| r.unwrap())),
            alloc_size,
            parse_fn
        )
    }

    /// Removes used bytes from buffer, and shifts everything to start.
    /// Returns remaining buffer bytes when at EOF.
    fn resize_no_alloc(&mut self) -> Result<Option<Vec<u8>>, Error> {
        let full_len = self.buffer.len();
        self.buffer.drain(0..self.parse_location);
        self.buffer.resize_with(full_len, || 0);
        self.read_end_location -= self.parse_location;
        // let num_new_bytes = self
        //     .reader
        //     .read(&mut self.buffer[(full_len - self.parse_location)..])?;
        let mut num_new_bytes = 0;
        for i in (&mut self.buffer[(full_len - self.parse_location)..]).iter_mut() {
            match self.source.next() {
                Some(b) => {
                    *i = b;
                    num_new_bytes += 1;
                },
                None => continue,
            }
        }
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
    /// Returns remaining buffer bytes when at EOF.
    fn resize_alloc(&mut self) -> Result<Option<Vec<u8>>, Error> {
        let old_len = self.buffer.len();
        self.buffer.resize_with(old_len + self.alloc_size, || 0);
        // let num_new_bytes = self.reader.read(&mut self.buffer[old_len..])?;
        let mut num_new_bytes = 0;
        for i in (&mut self.buffer[old_len..]).iter_mut() {
            match self.source.next() {
                Some(b) => {
                    *i = b;
                    num_new_bytes += 1;
                },
                None => continue,
            }
        }
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

impl<T, F> Iterator for ByteMuncher<T, F>
where
    F: Fn(&[u8], bool) -> Result<MunchOutput<T>, Error>,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.complete {
            return None;
        }
        loop {
            let parse_result = (self.parse_fn)(&self.buffer[self.parse_location..], false);
            match parse_result {
                // Complete parse
                Ok(Some((item, n))) => {
                    self.parse_location += n;
                    return Some(item);
                }
                // Handle incomplete parse
                Ok(None) => {
                    // Partial buffer not enough, shift existing and fill
                    if self.parse_location != 0 {
                        match self.resize_no_alloc() {

                            Ok(r) => {
                                // EOF
                                if let Some(last) = r {
                                    match (self.parse_fn)(&last, true) {
                                        Ok(Some((item, _))) => return Some(item),
                                        _ => return None,
                                    }
                                }
                            }
                            Err(e) => eprintln!("Error while resizing: {:?}", e),
                        }
                    // Entire buffer not enough, increase buffer size and refill
                    } else {
                        match self.resize_alloc() {
                            Ok(r) => {
                                // EOF
                                if let Some(last) = r {
                                    match (self.parse_fn)(&last, true) {
                                        Ok(Some((item, _))) => return Some(item),
                                        _ => return None,
                                    }
                                }
                            }
                            Err(e) => eprintln!("Error while resizing: {:?}", e),
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error while parsing: {:?}", e);
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

        let bytes = vec![0xff, 1, 0xff, 2, 2, 0xff, 3, 3, 3];

        let munched: Vec<Vec<u8>> = ByteMuncher::new(Box::new(bytes.into_iter()), 5, my_parse_function).collect();

        assert_eq!(munched.len(), 3);
        assert_eq!(munched[0], vec![0xff, 1]);
        assert_eq!(munched[1], vec![0xff, 2, 2]);
        assert_eq!(munched[2], vec![0xff, 3, 3, 3]);
    }

    /// Tests passing closure as parser function.
    #[test]
    pub fn closure() {

        let read = vec![0xff, 1, 0xff, 2, 2, 0xff, 3, 3, 3];

        let munched: Vec<Vec<u8>> = ByteMuncher::new(Box::new(read.into_iter()), 5, |b, _| {
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

    /// Test using Read as source.
    #[test]
    pub fn read() {

        use std::io::Cursor;
        
        let read = Cursor::new(vec![0xff, 1, 0xff, 2, 2, 0xff, 3, 3, 3]);

        let munched: Vec<Vec<u8>> = ByteMuncher::from_read(Box::new(read), 5, |b, _| {
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
