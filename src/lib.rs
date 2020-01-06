//! Provides endless byte stream parsing, compatible with nom v5.

use thiserror::Error;
use nom::{Err, IResult};
use std::io::Read;

/// Type alias for parse function
pub type ParseFn = dyn Fn(&[u8]) -> IResult<&[u8], &[u8]>;

/// Error types for this library
#[derive(Debug, Error)]
pub enum MunchError {
    /// Error while reading
    #[error("Error reading from Read object")]
    ReadError(#[from] std::io::Error),
}

/// Continuous reads bytes from a `Read` implementor,
/// parses that byte stream with the provided parse function,
/// and iterates over the parsed slices.
///
pub struct ReadMuncher<'a, 'b> {
    reader: &'a mut dyn Read,
    buffer: Vec<u8>,
    parse_location: usize,
    alloc_size: usize,
    parse_fn: &'b ParseFn, //Box<ParseFn>,
    complete: bool,
    read_end_location: usize,
}

impl<'a, 'b> ReadMuncher<'a, 'b> {
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
    pub fn new(reader: &'a mut dyn Read, alloc_size: usize, parse_fn: &'b ParseFn) -> Self {
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
}

impl<'a, 'b> Iterator for ReadMuncher<'a, 'b> {
    type Item = Vec<u8>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.complete == true {
            return None;
        }
        loop {
            match (self.parse_fn)(&self.buffer[self.parse_location..]) {
                Ok((_, blob)) => {
                    self.parse_location += blob.len();
                    return Some(blob.to_owned());
                }
                Err(Err::Incomplete(_)) => {
                    if self.parse_location != 0 {
                        match resize_no_alloc(self) {
                            Ok(r) => {
                                if r.is_some() {
                                    return r;
                                }
                            }
                            Err(_) => return None,
                        }
                    } else {
                        match resize_alloc(self) {
                            Ok(r) => {
                                if r.is_some() {
                                    return r;
                                }
                            }
                            Err(_) => return None,
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Malformed input. {:?}", e);
                    return None;
                }
            }
        }
    }
}

/// Removes used bytes from buffer, and shifts everything to start.
fn resize_no_alloc(muncher: &mut ReadMuncher) -> Result<Option<Vec<u8>>, MunchError> {
    let full_len = muncher.buffer.len();
    muncher.buffer.drain(0..muncher.parse_location);
    muncher.buffer.resize_with(full_len, || 0);
    muncher.read_end_location -= muncher.parse_location;
    let num_new_bytes = muncher
        .reader
        .read(&mut muncher.buffer[(full_len - muncher.parse_location)..])?;
    muncher.read_end_location += num_new_bytes;
    if num_new_bytes == 0 {
        let blob = &muncher.buffer[..muncher.read_end_location];
        muncher.complete = true;
        Ok(Some(blob.to_owned()))
    } else {
        muncher.parse_location = 0;
        Ok(None)
    }
}

/// Extends the buffer to allow storing more bytes.
fn resize_alloc(muncher: &mut ReadMuncher) -> Result<Option<Vec<u8>>, MunchError> {
    let old_len = muncher.buffer.len();
    muncher
        .buffer
        .resize_with(old_len + muncher.alloc_size, || 0);
    let num_new_bytes = muncher.reader.read(&mut muncher.buffer[old_len..])?;
    muncher.read_end_location += num_new_bytes;
    if num_new_bytes == 0 {
        let blob = &muncher.buffer[..muncher.read_end_location];
        muncher.complete = true;
        Ok(Some(blob.to_owned()))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use nom::bytes::streaming;
    use std::io::Cursor;

    const TAG_BYTES: [u8; 3] = [1, 2, 3];

    fn parse_one_blob<'a>(input: &'a [u8]) -> IResult<&'a [u8], &'a [u8]> {
        // println!("Entered: parse_one_blob({:?})", input);
        let (i, tag) = streaming::tag(&TAG_BYTES)(input)?;
        let (remain, rest) = streaming::take_until(&TAG_BYTES[..])(i)?;
        let blob_len = tag.len() + rest.len();
        let blob = &input[..blob_len];
        Ok((remain, blob))
    }

    #[test]
    pub fn run_iter() {
        println!("TEST - run_iter:");
        let mut file = Cursor::new(vec![
            1u8, 2, 3, 11, 11, 11, 1, 2, 3, 22, 22, 22, 22, 22, 22, 22, 1, 2, 3, 33, 33, 33, 33, 1,
            2, 3, 44, 44, 44, 1, 2, 3, 55, 55, 55, 55, 55, 55, 1, 2, 3, 66, 1u8, 2, 3, 77, 77, 1,
            2, 3, 88, 88, 88, 88, 88, 88, 88, 1, 2, 3, 99, 99, 99, 99,
        ]);

        let muncher = ReadMuncher::new(&mut file, 5, &parse_one_blob);

        for blob in muncher {
            println!("Found blob: {:?}", blob);
        }
    }

    #[test]
    pub fn doc_test() {
        println!("doc_test");
        use nom::bytes::streaming::*;
        use nom::{Err, IResult, Needed};
        use std::io::Cursor;
        use std::result::Result;

        fn my_parse_function(input: &[u8]) -> IResult<&[u8], &[u8]> {
            let (i, tag) = streaming::tag(&[0xff])(input)?;
            let (remain, rest) = streaming::take_until(&[0xff][..])(i)?;
            let blob_len = tag.len() + rest.len();
            let blob = &input[..blob_len];
            Ok((remain, blob))
        }

        let mut read = Cursor::new(vec![0xff, 1, 0xff, 2, 2, 0xff, 3, 3, 3]);

        let mut munched: Vec<Vec<u8>> = vec![];
        let muncher = ReadMuncher::new(&mut read, 5, &my_parse_function);

        for m in muncher {
            println!("Found: {:?}", m);
            munched.push(m);
        }

        assert_eq!(munched.len(), 3);
        assert_eq!(munched[0], vec![0xff, 1]);
        assert_eq!(munched[1], vec![0xff, 2, 2]);
        assert_eq!(munched[2], vec![0xff, 3, 3, 3]);
    }
}
