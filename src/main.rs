use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use clap::{Parser, Subcommand};
use crate::storage::{load_chain, save_chain};

mod storage;

#[derive(Parser)]
#[command(author, version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Mine { data: String },
    Show,
    Validate,
}

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

    pub fn compute_hash(&self) -> String {
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

#[derive(Serialize, Deserialize)]
pub struct Blockchain {
    pub chain: Vec<Block>,
    pub difficulty: usize,
}

impl Blockchain {
    pub fn new(difficulty: usize) -> Self {
        let mut genesis = Block::new(0, "Genesis".into(), "0".into());
        genesis.mine(difficulty);
        Self {
            chain: vec![genesis],
            difficulty,
        }
    }

    pub fn add_block(&mut self, block: Block) -> Result<(), String> {
        let last = self.chain.last().unwrap();
        if block.prev_hash != last.hash {
            return Err("prev_hash mismatch".into());
        }
        if block.index != last.index + 1 {
            return Err("index mismatch".into());
        }
        if block.compute_hash() != block.hash {
            return Err("hash invalid".into());
        }
        if !block.hash.starts_with(&"0".repeat(self.difficulty)) {
            return Err("proof-of-work invalid".into());
        }
        self.chain.push(block);
        Ok(())
    }

    pub fn is_valid(&self) -> bool {
        for (i, block) in self.chain.iter().enumerate().skip(1) {
            let prev = &self.chain[i - 1];
            if block.prev_hash != prev.hash {
                return false;
            }
            if block.compute_hash() != block.hash {
                return false;
            }
            if !block.hash.starts_with(&"0".repeat(self.difficulty)) {
                return false;
            }
        }
        true
    }
}
fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let path = "./husaynex-chain.txt";
    let mut blockchain = match load_chain(path) {
        Ok(chain) => chain,
        Err(e) => {
            eprintln!("Error loading blockchain from {}: {}. Creating a new one.", path, e);
            Blockchain::new(2)
        }
    };

    match cli.command {
        Commands::Mine { data } => {
            let last = blockchain.chain.last().unwrap();
            let mut block = Block::new(last.index + 1, data, last.hash.clone());
            block.mine(2);
            match blockchain.add_block(block) {
                Ok(()) => println!("Block mined and added to the blockchain!"),
                Err(e) => println!("Failed to add block: {}", e),
            }
            save_chain(path, &blockchain)?;
        }
        Commands::Show => {
            for block in &blockchain.chain {
                println!("{:#?}", block);
            }
        }
        Commands::Validate => {
            if blockchain.is_valid() {
                println!("Blockchain is valid!")
            } else if !blockchain.is_valid() {
                println!("Blockchain is invalid")
            }
        }
    }

    Ok(())
}
