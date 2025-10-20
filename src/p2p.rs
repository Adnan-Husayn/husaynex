use crate::{Block, Blockchain};
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::result::Result::{Err, Ok};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Message {
    GetChain,
    SendChain(Vec<Block>),
    SendBlock(Block),
    RequestBlock(u64),
    NewBlock(Block),
    Error(String),
}

#[derive(Clone)]
pub struct PeerState {
    pub blockchain: Arc<Mutex<Blockchain>>,
    pub known_peers: Arc<Mutex<HashSet<SocketAddr>>>,
    pub new_block_sender: broadcast::Sender<Block>,
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
    let node_id_prefix = state.blockchain.lock().unwrap().chain[0]
        .hash
        .chars()
        .take(8)
        .collect::<String>();
    println!(
        "[P2P Handler {} {}] Connection established.",
        node_id_prefix, remote_addr
    );

    let mut new_block_receiver = state.new_block_sender.subscribe();

    loop {
        tokio::select! {

            incoming_message_result = recv_message(&mut reader) => {
                let incoming_message = match incoming_message_result {
                    Ok(Some(msg)) => msg,
                    Ok(None) => {
                        println!("[P2P Handler {} {}] Peer disconnected gracefully.", node_id_prefix, remote_addr);
                        break;
                    }
                    Err(e) => {
                        eprintln!(
                            "[P2P Handler {} {}] Failed to receive message: {}. Closing connection.",
                            node_id_prefix, remote_addr, e
                        );

                        let error_response = Message::Error(format!("Invalid message format or network error: {}", e));
                        if let Err(e) = send_message(&mut writer, &error_response).await {
                             eprintln!("[P2P Handler {} {}] Failed to send error message: {:?}", node_id_prefix, remote_addr, e);
                        }
                        break;
                    }
                };

                println!("[P2P Handler {} {}] Received: {:?}", node_id_prefix, remote_addr, incoming_message);

                let response_message = match incoming_message {
                    Message::GetChain => {
                        let blockchain = state.blockchain.lock().unwrap();
                        Message::SendChain(blockchain.chain.clone())
                    }
                    Message::SendChain(received_chain) => {
                        let mut blockchain = state.blockchain.lock().unwrap();
                        if received_chain.len() > blockchain.chain.len() && Blockchain::is_valid_static(&received_chain, blockchain.difficulty) {
                            println!(
                                "[P2P Handler {} {}] Adopting a longer valid chain (length {} vs {}).",
                                node_id_prefix,
                                remote_addr,
                                received_chain.len(),
                                blockchain.chain.len()
                            );
                            blockchain.chain = received_chain;

                            Message::Error("Adopted new chain successfully.".into())
                        } else {
                            println!(
                                "[P2P Handler {} {}] Received chain not longer or not valid (length {} vs {}). Not adopting.",
                                node_id_prefix,
                                remote_addr,
                                received_chain.len(),
                                blockchain.chain.len()
                            );
                            Message::Error("Received chain not adopted.".into())
                        }
                    }
                    Message::RequestBlock(index) => {
                        let blockchain = state.blockchain.lock().unwrap();
                        if let Some(block) = blockchain.chain.get(index as usize) {
                            Message::SendBlock(block.clone())
                        } else {
                            println!(
                                "[P2P Handler {} {}] Requested block index {} not found.",
                                node_id_prefix, remote_addr, index
                            );
                            Message::Error(format!("Block with index {} not found.", index))
                        }
                    }
                    Message::SendBlock(block) => {
                        println!(
                            "[P2P Handler {} {}] Received block {} from peer. (Not yet integrating, needs advanced logic).",
                            node_id_prefix, remote_addr, block.index
                        );
                        Message::Error("Received block but not yet integrated.".into())
                    }
                    Message::NewBlock(block) => {
                        let mut blockchain = state.blockchain.lock().unwrap();

                        let last_block = blockchain.chain.last().unwrap();
                        if block.prev_hash == last_block.hash && block.index == last_block.index + 1
                           && block.compute_hash() == block.hash
                           && block.hash.starts_with(&"0".repeat(blockchain.difficulty)) {
                            println!("[P2P Handler {} {}] Received valid new block {} from peer. Adding to chain.", node_id_prefix, remote_addr, block.index);
                            blockchain.chain.push(block.clone());




                            Message::Error(format!("Acknowledged and added new block {}.", block.index))
                        } else {
                            println!("[P2P Handler {} {}] Received invalid new block {} from peer. Ignoring.", node_id_prefix, remote_addr, block.index);
                            Message::Error(format!("Received invalid new block {}.", block.index))
                        }
                    }
                    Message::Error(e) => {
                        eprintln!("[P2P Handler {} {}] Peer sent an error: {}", node_id_prefix, remote_addr, e);
                        Message::Error(format!("Acknowledged peer error: {}", e))
                    }
                };


                if let Message::Error(s) = &response_message {

                    if !s.starts_with("Acknowledged") && !s.starts_with("Adopted") {
                         println!("[P2P Handler {} {}] Sent: {:?}", node_id_prefix, remote_addr, response_message);
                    }
                } else {
                    println!("[P2P Handler {} {}] Sent: {:?}", node_id_prefix, remote_addr, response_message);
                }
                send_message(&mut writer, &response_message).await?;
            }


            new_block_result = new_block_receiver.recv() => {
                match new_block_result {
                    Ok(block) => {
                        println!("[P2P Handler {} {}] Broadcasting locally mined block {} to peer.", node_id_prefix, remote_addr, block.index);
                        if let Err(e) = send_message(&mut writer, &Message::NewBlock(block)).await {
                            eprintln!("[P2P Handler {} {}] Failed to broadcast new block: {:?}", node_id_prefix, remote_addr, e);


                        }
                    }


                      Err(RecvError::Lagged(num_skipped)) => {
                        eprintln!("[P2P Handler {} {}] New block receiver lagged, skipped {} messages.", node_id_prefix, remote_addr, num_skipped);
                    }

                    Err(RecvError::Closed) => {
                        println!("[P2P Handler {} {}] New block sender closed, shutting down broadcast for this peer.", node_id_prefix, remote_addr);
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

const PEER_SYNC_INTERVAL_SECS: u64 = 30;

pub async fn connect_to_peer(peer_addr: SocketAddr, state: PeerState) -> anyhow::Result<()> {
    let node_id_prefix = state.blockchain.lock().unwrap().chain[0]
        .hash
        .chars()
        .take(8)
        .collect::<String>();
    println!(
        "[P2P Client {}] Attempting to connect to peer: {}",
        node_id_prefix, peer_addr
    );
    let mut stream = TcpStream::connect(peer_addr).await?;
    println!(
        "[P2P Client {}] Connected to peer: {}",
        node_id_prefix, peer_addr
    );

    let (mut reader, mut writer) = stream.split();

    let request_chain = Message::GetChain;
    send_message(&mut writer, &request_chain).await?;
    println!(
        "[P2P Client {}] Sent GET_CHAIN to {}",
        node_id_prefix, peer_addr
    );

    loop {
        let message_option = recv_message(&mut reader).await;

        let message = match message_option {
            Ok(Some(msg)) => msg,
            Ok(None) => {
                println!(
                    "[P2P Client {}] Peer {} disconnected during chain exchange.",
                    node_id_prefix, peer_addr
                );
                break;
            }
            Err(e) => {
                eprintln!(
                    "[P2P Client {}] Error receiving response from {}: {}. Closing connection.",
                    node_id_prefix, peer_addr, e
                );
                break;
            }
        };

        println!(
            "[P2P Client {}] Received from {}: {:?}",
            node_id_prefix, peer_addr, message
        );

        if let Message::SendChain(received_chain) = message {
            let mut blockchain = state.blockchain.lock().unwrap();
            if received_chain.len() > blockchain.chain.len()
                && Blockchain::is_valid_static(&received_chain, blockchain.difficulty)
            {
                println!(
                    "[P2P Client {}] Adopted a longer valid chain from {} (length {} vs {}).",
                    node_id_prefix,
                    peer_addr,
                    received_chain.len(),
                    blockchain.chain.len()
                );
                blockchain.chain = received_chain;
            } else {
                println!(
                    "[P2P Client {}] Received chain from {} is not longer or not valid. Not adopting.",
                    node_id_prefix, peer_addr
                );
            }
            break;
        } else if let Message::Error(e) = message {
            eprintln!(
                "[P2P Client {}] Peer {} reported error: {}",
                node_id_prefix, peer_addr, e
            );
            break;
        } else {
            println!(
                "[P2P Client {}] Received unexpected message from {}: {:?}",
                node_id_prefix, peer_addr, message
            );
            break;
        }
    }
    Ok(())
}

pub async fn discover_and_sync_peers(
    state: PeerState,
    initial_peers: Vec<String>,
) -> anyhow::Result<()> {
    {
        let mut known_peers = state.known_peers.lock().unwrap();
        for peer_str in initial_peers {
            if let Ok(addr) = peer_str.parse::<SocketAddr>() {
                if addr
                    != state.blockchain.lock().unwrap().chain[0]
                        .hash
                        .parse::<SocketAddr>()
                        .unwrap_or_else(|_| "0.0.0.0:0".parse().unwrap())
                {
                    known_peers.insert(addr);
                }
            } else {
                eprintln!("[P2P Discovery] Invalid initial peer address: {}", peer_str);
            }
        }
    }

    let node_id_prefix = state.blockchain.lock().unwrap().chain[0]
        .hash
        .chars()
        .take(8)
        .collect::<String>();

    loop {
        println!(
            "[P2P Discovery {}] Starting peer sync cycle...",
            node_id_prefix
        );
        let peer_to_connect: Vec<SocketAddr> =
            { state.known_peers.lock().unwrap().iter().cloned().collect() };

        let mut tasks = FuturesUnordered::new();

        if peer_to_connect.is_empty() {
            println!(
                "[P2P Discovery {}] No known peers to connect to.",
                node_id_prefix
            );
        }

        for peer_addr in peer_to_connect {
            let state_clone = state.clone();

            tasks.push(tokio::spawn(async move {
                if let Err(e) = connect_to_peer(peer_addr, state_clone.clone()).await {
                    eprintln!(
                        "[P2P Discovery {}] Error connecting to peer {}: {:?}",
                        state_clone.blockchain.lock().unwrap().chain[0]
                            .hash
                            .chars()
                            .take(8)
                            .collect::<String>(),
                        peer_addr,
                        e
                    );
                }
            }));
        }

        while let Some(result) = tasks.next().await {
            if let Err(e) = result {
                eprintln!(
                    "[P2P Discovery {}] Peer connection task failed: {:?}",
                    node_id_prefix, e
                );
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

async fn send_message(
    writer: &mut (impl AsyncWriteExt + Unpin),
    message: &Message,
) -> anyhow::Result<()> {
    let serialized_payload = serde_json::to_vec(message)?;
    let len = serialized_payload.len() as u32;

    writer.write_all(&len.to_le_bytes()).await?;
    writer.write_all(&serialized_payload).await?;
    writer.flush().await?;
    Ok(())
}

async fn recv_message(reader: &mut (impl AsyncReadExt + Unpin)) -> anyhow::Result<Option<Message>> {
    let mut len_bytes = [0u8; 4];

    if reader.read_exact(&mut len_bytes).await.is_err() {
        return Ok(None);
    }

    let len = u32::from_le_bytes(len_bytes) as usize;

    if len == 0 {
        return Ok(None);
    }

    let mut payload_buffer = vec![0u8; len];
    reader.read_exact(&mut payload_buffer).await?;

    let message: Message = serde_json::from_slice(&payload_buffer)?;
    Ok(Some(message))
}
