use crate::{Block, Blockchain};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt::format;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

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
