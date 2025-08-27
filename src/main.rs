use std::env;

use miden_client::rpc::Endpoint;

pub mod faucet;
pub mod server;
pub mod tx_worker;
pub mod utils;

#[tokio::main]
async fn main() {
    let command = env::args().nth(1).unwrap_or_default();
    match command.as_str() {
        "start-server" => {
            if let Err(e) = crate::server::start_server().await {
                eprintln!("Failed to start server: {}", e);
            }
        }
        "create-faucet" => {
            let network = env::args().nth(2).unwrap_or_else(|| "testnet".to_string());
            let endpoint = match network.as_str() {
                "testnet" => Endpoint::testnet(),
                "localnet" => Endpoint::localhost(),
                _ => {
                    eprintln!("Unknown network: {}. Use 'testnet' or 'mainnet'.", network);
                    return;
                }
            };
            if let Err(e) = crate::faucet::create_new_faucet(endpoint).await {
                eprintln!("Failed to create faucet: {}", e);
            }
        }
        _ => {
            eprintln!("Unknown command: {}", command);
            eprintln!("Usage: {} <start-server>", env::args().next().unwrap());
        }
    };
}
