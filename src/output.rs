use std::io::{self, Write};

pub fn write_json<T: serde::Serialize>(output: &mut impl Write, value: &T) -> io::Result<()> {
    serde_json::to_writer(&mut *output, value).map_err(io::Error::other)?;
    output.write_all(b"\n")
}
