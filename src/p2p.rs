use crate::{Block, Blockchain};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Message {
    GetChain,
    SendChain(Vec<Block>),
    SendBlock(Block),
    RequestBlock(u64)
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
            //handle connection
        });
    }
}
