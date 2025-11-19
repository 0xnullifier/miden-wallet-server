use std::env;

use miden_client::rpc::Endpoint;

use miden_faucet_server::{
    faucet,
    server::{self},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    let command = env::args().nth(1).unwrap_or_default();
    match command.as_str() {
        "start-server" => {
            server::start_server().await?;
        }
        "create-faucet" => {
            let network = env::args().nth(2).unwrap_or_else(|| "testnet".to_string());
            let endpoint = match network.as_str() {
                "testnet" => Endpoint::testnet(),
                "localnet" => Endpoint::localhost(),
                "devnet" => Endpoint::devnet(),
                _ => {
                    eprintln!("Unknown network: {}. Use 'testnet' or 'mainnet'.", network);
                    return Ok(());
                }
            };
            faucet::create_new_faucet(endpoint).await?
        }
        _ => {
            eprintln!("Unknown command: {}", command);
            eprintln!("Usage: {} <start-server>", env::args().next().unwrap());
        }
    };

    Ok(())
}
