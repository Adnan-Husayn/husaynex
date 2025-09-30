use std::{fs::File, io::{self, Read}};
use crate::Blockchain;
use anyhow::Ok;
use serde_json::{from_reader, to_writer_pretty};

pub fn save_chain(path: &str, chain: &Blockchain) -> anyhow::Result<()> {
    let f = File::create(path)?;
    to_writer_pretty(f, chain)?;
    Ok(())
}

pub fn load_chain(path: &str) -> anyhow::Result<Blockchain> {
    let mut f = File::open(path)?;
    let mut buffer = String::new();
    f.read_to_string(&mut buffer)?;
    if buffer.is_empty() {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "File is empty").into());
    }
    let chain = from_reader(buffer.as_bytes())?;
    Ok(chain)
}