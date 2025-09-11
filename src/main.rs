use std::env;

use futures::executor::block_on;
use miden_client::{
    account::Address, asset::FungibleAsset, note::NoteType, rpc::Endpoint,
    transaction::TransactionRequestBuilder,
};

use miden_faucet_server::{
    faucet,
    server::{self, FAUCET_ID},
    utils::init_client_and_prover,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
        "test-mint" => block_on(async {
            let (mut client, remote_prover) = init_client_and_prover().await;

            let fungible_asset = FungibleAsset::new(*FAUCET_ID, 10000000).unwrap();
            let (_, target_address) =
                Address::from_bech32("mdev1qzv5e0xp6n28rypelxdgqfz8secqzks5f77")
                    .expect("Invalid address format");
            let target_id = match target_address {
                Address::AccountId(id) => id.id(),
                _ => {
                    panic!("Target address is not an AccountId")
                }
            };
            let transaction_request = TransactionRequestBuilder::new()
                .build_mint_fungible_asset(
                    fungible_asset,
                    target_id,
                    NoteType::Public,
                    client.rng(),
                )
                .expect("Failed to build transaction request");
            let transaction_execution_result = client
                .new_transaction(*FAUCET_ID, transaction_request)
                .await
                .expect("Failed to execute transaction");
            let digest = transaction_execution_result.executed_transaction().id();
            client
                .submit_transaction_with_prover(transaction_execution_result, remote_prover)
                .await
                .expect("Failed to submit transaction");
            println!("Mint transaction submitted with digest: {:?}", digest);
        }),
        _ => {
            eprintln!("Unknown command: {}", command);
            eprintln!("Usage: {} <start-server>", env::args().next().unwrap());
        }
    };

    Ok(())
}
