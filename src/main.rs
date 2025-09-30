use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Block {
    pub index: u64,
    pub timestamp: DateTime<Utc>,
    pub data: String,
    pub nonce: u64,
    pub prev_hash: String,
    pub hash: String,
}

impl Block {
    pub fn new(index: u64, data: String, prev_hash: String) -> Self {
        Self {
            index,
            timestamp: Utc::now(),
            data,
            nonce: 0,
            prev_hash,
            hash: String::new(),
        }
    }

    pub  fn compute_hash(&self) -> String {
        let payload = format!(
            "{}{}{}{}{}",
            self.index,
            self.timestamp.to_rfc3339(),
            self.data,
            self.nonce,
            self.prev_hash
        );

        blake3::hash(payload.as_bytes()).to_hex().to_string()
    }

    pub fn mine(&mut self, difficulty: usize) {
        loop {
            let hash = self.compute_hash();
            if hash.starts_with(&"0".repeat(difficulty)) {
                self.hash = hash;
                break;
            }
            self.nonce = self.nonce.wrapping_add(1);
        }
    }
}

fn main() {}
