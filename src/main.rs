use std::{collections::HashSet, sync::{Arc, Mutex}, net::SocketAddr};

use crate::{p2p::{discover_and_sync_peers, start_p2p_server}, storage::{load_chain, save_chain}};
use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

mod storage;
mod p2p;

#[derive(Parser)]
#[command(author, version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    #[arg(long, default_value = "127.0.0.1:8000")]
    p2p_listen_addr:String,
    #[arg(long, value_delimiter = ',')]
    p2p_connect_to: Option<Vec<String>>
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
        Self::is_valid_static(&self.chain, self.difficulty)
    }

    pub fn is_valid_static(chain_to_validate: &[Block], difficulty: usize) -> bool {
        if chain_to_validate.is_empty() {
            return false; 
        }
        
        let genesis = &chain_to_validate[0];
        if genesis.index != 0 || genesis.prev_hash != "0" {
            eprintln!("Validation failed: Invalid genesis block.");
            return false;
        }
        if !genesis.hash.starts_with(&"0".repeat(difficulty)) || genesis.compute_hash() != genesis.hash {
            eprintln!("Validation failed: Genesis block proof-of-work or hash invalid.");
            return false;
        }


        for (i, block) in chain_to_validate.iter().enumerate().skip(1) {
            let prev = &chain_to_validate[i - 1];
            if block.prev_hash != prev.hash {
                eprintln!("Validation failed: prev_hash mismatch for block {}", block.index);
                return false;
            }
            if block.index != prev.index + 1 {
                eprintln!("Validation failed: index mismatch for block {}", block.index);
                return false;
            }
            if block.compute_hash() != block.hash {
                eprintln!("Validation failed: hash invalid for block {}", block.index);
                return false;
            }
            if !block.hash.starts_with(&"0".repeat(difficulty)) {
                eprintln!("Validation failed: proof-of-work invalid for block {}", block.index);
                return false;
            }
        }
        true
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let path = "./chain.json";
    let mut blockchain_data = match load_chain(path) {
        Ok(chain) => chain,
        Err(e) => {
            eprintln!(
                "Error loading blockchain from {}: {}. Creating a new one.",
                path, e
            );
            Blockchain::new(2)
        }
    };

    let blockchain = Arc::new(Mutex::new(blockchain_data));

    let known_peers = Arc::new(Mutex::new(HashSet::<SocketAddr>::new()));

    let (new_block_sender, _new_blockchain_receiver) = broadcast::channel(16);

    let peer_state = p2p::PeerState {
        blockchain: Arc::clone(&blockchain),
        known_peers: Arc::clone(&known_peers),
        new_block_sender: new_block_sender.clone(),
    };

    let p2p_server_handle = tokio::spawn(start_p2p_server(
        cli.p2p_listen_addr.clone(),
        peer_state.clone()
    ));
    println!("[Main] P2P server will listen on: {}", cli.p2p_listen_addr);

    let p2p_client_handle = tokio::spawn(discover_and_sync_peers(
        peer_state.clone(),
        cli.p2p_connect_to.unwrap_or_default(),
    ));
    println!("[Main] P2P client/discovery spawned.");

    match cli.command {
        Commands::Mine { data } => {
            let mut bc = blockchain.lock().unwrap();
            let last = bc.chain.last().unwrap();
            let mut block = Block::new(last.index+1, data, last.hash.clone());
            block.mine(bc.difficulty);
            match bc.add_block(block.clone()) {
                Ok(()) => {
                    println!("Block mined and added to the blockchain!");
                    if let Err(e) = new_block_sender.send(block) { 
                        eprintln!("[Main] Failed to broadcast new block: {:?}", e);
                    } else {
                        println!("[Main] Successfully broadcast new block to peers.");
                    }
                }
                Err(e) => println!("Failed to add block: {}", e)
            }
            save_chain(path, &bc)?;
            println!("[Main] Mining complete. Node will now remain active for P2P operations.");
            p2p_server_handle.await??;
        },
        Commands::Show => {
            let bc = blockchain.lock().unwrap();
            for block in &bc.chain {
                println!("{:#?}", block);
            }
        },
        Commands::Validate => {
            let bc = blockchain.lock().unwrap();
            if bc.is_valid() {
                println!("Blockchain is valid!")
            } else {
                println!("Blockchain is invalid")
            }
        }
    }

    Ok(())
}
