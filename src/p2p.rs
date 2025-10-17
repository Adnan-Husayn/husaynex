use crate::{Block, Blockchain};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use futures::stream::StreamExt;
use futures::stream::FuturesUnordered;
use std::result::Result::{Ok, Err};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Message {
    GetChain,
    SendChain(Vec<Block>),
    SendBlock(Block),
    RequestBlock(u64),
    Error(String),
}

#[derive(Clone)]
pub struct PeerState {
    pub blockchain: Arc<Mutex<Blockchain>>,
    pub known_peers: Arc<Mutex<HashSet<SocketAddr>>>,
}

pub async fn start_p2p_server(addr: String, state: PeerState) -> anyhow::Result<()> {
    let listener = TcpListener::bind(&addr).await?;
    println!("P2P server listening on {}", addr);

    loop {
        let (socket, remote_addr) = listener.accept().await?;
        println!("New connection from: {}", remote_addr);
        let peer_state_clone = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(socket, peer_state_clone, remote_addr).await {
                eprintln!(
                    "[P2P] Error handling connection from {}: {:?}",
                    remote_addr, e
                )
            }
        });
    }
}

const PEER_BUFFER_SIZE: usize = 8192;

async fn handle_connection(
    mut socket: TcpStream,
    state: PeerState,
    remote_addr: SocketAddr,
) -> anyhow::Result<()> {
    let (mut reader, mut writer) = socket.split();
    let mut buffer = Vec::with_capacity(PEER_BUFFER_SIZE);

    println!("[P2P Handler {}] Connection established.", remote_addr);

    loop {
        buffer.clear();

        let bytes_read = reader.read_buf(&mut buffer).await?;
        if bytes_read == 0 {
            println!("[P2P Handler {}] Peer disconnected.", remote_addr);
            break;
        }

        let incoming_message: Message = match serde_json::from_slice(&buffer[..bytes_read]) {
            Ok(msg) => msg,
            Err(e) => {
                eprintln!(
                    "[P2P Handler {}] Failed to deserialize message: {}. Raw bytes: {:?}",
                    remote_addr,
                    e,
                    &buffer[..bytes_read]
                );

                let error_response = Message::Error(format!("Invalid message format: {}", e));
                let serialized_error = serde_json::to_vec(&error_response)?;
                writer.write_all(&serialized_error).await?;
                writer.flush().await?;
                continue;
            }
        };

        println!("[P2P Handler {}] Received: {:?}", remote_addr, incoming_message);

        let response_message = match incoming_message {
            Message::GetChain => {
                let blockchain = state.blockchain.lock().unwrap();
                Message::SendChain(blockchain.chain.clone())
            },
            Message::SendChain(received_chain) => {
                let mut blockchain = state.blockchain.lock().unwrap();

                if received_chain.len() > blockchain.chain.len() && Blockchain::is_valid_static(&received_chain, blockchain.difficulty) {
                    println!(
                        "[P2P Handler {}] Adopting a longer valid chain (length {} vs {}).",
                        remote_addr,
                        received_chain.len(),
                        blockchain.chain.len()
                    );

                    blockchain.chain = received_chain;
                    Message::Error("Adopted new chain successfully (no further action needed).".into())
                }
                else {
                    println!(
                        "[P2P Handler {}] Received chain not longer or not valid (length {} vs {}). Not adopting.",
                        remote_addr,
                        received_chain.len(),
                        blockchain.chain.len()
                    );
                    Message::Error("Received chain not adopted.".into())
                }
            },
            Message::RequestBlock(index) => {
                let blockchain = state.blockchain.lock().unwrap();
                if let Some(block) = blockchain.chain.get(index as usize) {
                    Message::SendBlock(block.clone())
                } else {
                     println!(
                        "[P2P Handler {}] Requested block index {} not found.",
                        remote_addr, index
                    );
                    Message::Error(format!("Block with index {} not found.", index))
                }
            },
            Message::SendBlock(block) => {
                println!(
                    "[P2P Handler {}] Received block {} from peer. (Not yet integrating, needs advanced logic).",
                    remote_addr, block.index
                );
                Message::Error("Received block but not yet integrated.".into())
            },
            Message::Error(e) => {
                eprintln!("[P2P Handler {}] Peer sent an error: {}", remote_addr, e);
                Message::Error("Acknowledged peer error.".into())
            }
        };

        let serialized_response = serde_json::to_vec(&response_message)?;
        writer.write_all(&serialized_response).await?;
        writer.flush().await?;
        println!("[P2P Handler {}] Sent: {:?}", remote_addr, response_message);
    }

    Ok(())
}

const PEER_SYNC_INTERVAL_SECS: u64 = 30;

pub async fn connect_to_peer(
    peer_addr: SocketAddr,
    state: PeerState
) -> anyhow::Result<()> {
    println!("[P2P Client {}] Attempting to connect to peer: {}", state.blockchain.lock().unwrap().chain[0].hash, peer_addr);
    let mut stream = TcpStream::connect(peer_addr).await?;
    println!("[P2P Client {}] Connected to peer: {}", state.blockchain.lock().unwrap().chain[0].hash, peer_addr);

    let (mut reader, mut writer) = stream.split();

    let request_chain = Message::GetChain;
    let serialized_request = serde_json::to_vec(&request_chain)?;
    writer.write_all(&serialized_request).await?;
    writer.flush().await?;
    println!("[P2P Client {}] Sent GET_CHAIN to {}", state.blockchain.lock().unwrap().chain[0].hash, peer_addr);

    let mut buffer = Vec::with_capacity(PEER_BUFFER_SIZE);

    loop {
        let bytes_read = reader.read_buf(&mut buffer).await?;
        if bytes_read == 0 {
            println!("[P2P Client {}] Peer {} disconnected during chain exchange.", state.blockchain.lock().unwrap().chain[0].hash, peer_addr);
            break;
        }

        let message : Message = match serde_json::from_slice(&buffer[..bytes_read]) {
            Ok(msg) => msg,
            Err(e) => {
                 eprintln!(
                    "[P2P Client {}] Error deserializing response from {}: {}",
                    state.blockchain.lock().unwrap().chain[0].hash,
                    peer_addr,
                    e
                );
                buffer.clear(); 
                continue;
            }
        };
        
        println!("[P2P Client {}] Received from {}: {:?}", state.blockchain.lock().unwrap().chain[0].hash, peer_addr, message);

        if let Message::SendChain(received_chain) = message {
            let mut blockchain = state.blockchain.lock().unwrap();
            if received_chain.len() > blockchain.chain.len() && Blockchain::is_valid_static(&received_chain, blockchain.difficulty) {
                println!(
                    "[P2P Client {}] Adopted a longer valid chain from {} (length {} vs {}).",
                    state.blockchain.lock().unwrap().chain[0].hash,
                    peer_addr,
                    received_chain.len(),
                    blockchain.chain.len()
                );
                blockchain.chain = received_chain;
            } else {
                println!(
                    "[P2P Client {}] Received chain from {} is not longer or not valid. Not adopting.",
                    state.blockchain.lock().unwrap().chain[0].hash,
                    peer_addr
                );
            }
            break;
        } else if let Message::Error(e) = message {
            eprintln!("[P2P Client {}] Peer {} reported error: {}", state.blockchain.lock().unwrap().chain[0].hash, peer_addr, e);
            break;
        } else {
            println!("[P2P Client {}] Received unexpected message from {}: {:?}", state.blockchain.lock().unwrap().chain[0].hash, peer_addr, message);
            break;
        }
    }
    Ok(())
}

pub async fn discover_and_sync_peers(state: PeerState, initial_peers: Vec<String>) -> anyhow::Result<()> {
    {
        let mut known_peers = state.known_peers.lock().unwrap();
        for peer_str in initial_peers {
            if let Ok(addr) = peer_str.parse::<SocketAddr>() {
                if addr != state.blockchain.lock().unwrap().chain[0].hash.parse::<SocketAddr>().unwrap_or_else(|_| "0.0.0.0:0".parse().unwrap()) {
                    known_peers.insert(addr);
                }
            } else {
                eprintln!("[P2P Discovery] Invalid initial peer address: {}", peer_str);
            }
        }
    }

    loop {
        println!("[P2P Discovery {}] Starting peer sync cycle...", state.blockchain.lock().unwrap().chain[0].hash);
        let peer_to_connect: Vec<SocketAddr> = {
            state.known_peers.lock().unwrap().iter().cloned().collect()
        };

        let mut tasks = FuturesUnordered::new();

        if peer_to_connect.is_empty() {
            println!("[P2P Discovery {}] No known peers to connect to.", state.blockchain.lock().unwrap().chain[0].hash);
        }

        for peer_addr in peer_to_connect {
            let state_clone = state.clone();

            tasks.push(tokio::spawn(async move {
                if let Err(e) = connect_to_peer(peer_addr, state_clone.clone()).await {
                    eprintln!("[P2P Discovery {}] Error connecting to peer {}: {:?}", state_clone.blockchain.lock().unwrap().chain[0].hash, peer_addr, e);
                }
            }));
        }

        while let Some(result) = tasks.next().await {
            if let Err(e) = result {
                eprintln!("[P2P Discovery {}] Peer connection task failed: {:?}", state.blockchain.lock().unwrap().chain[0].hash, e);
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(PEER_SYNC_INTERVAL_SECS)).await;
    }
}

impl PeerState {
    pub fn is_self_address(&self, addr: &SocketAddr) -> bool {
        let listen_addr_str = "0.0.0.0:0";
        addr.to_string() == listen_addr_str
    }
}