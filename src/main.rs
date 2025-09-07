use std::{env, sync::Arc, time::Duration};

use futures::executor::block_on;
use miden_client::{
    Client, Felt,
    account::{
        AccountBuilder, AccountId, AccountIdAddress, AccountStorageMode, AccountType, Address,
        AddressInterface, NetworkId,
        component::{AuthRpoFalcon512, BasicFungibleFaucet, BasicWallet},
    },
    asset::{FungibleAsset, TokenSymbol},
    auth::AuthSecretKey,
    builder::ClientBuilder,
    crypto::SecretKey,
    keystore::FilesystemKeyStore,
    note::NoteType,
    rpc::{Endpoint, TonicRpcClient},
    transaction::TransactionRequestBuilder,
};
use rand::{RngCore, rngs::StdRng};

use crate::{server::FAUCET_ID, utils::init_client_and_prover};

pub mod faucet;
pub mod server;
pub mod tx_worker;
pub mod utils;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let command = env::args().nth(1).unwrap_or_default();
    match command.as_str() {
        "start-server" => crate::server::start_server().await?,
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
            crate::faucet::create_new_faucet(endpoint).await?
        }
        "test-tokio-mint" => block_on(async {
            let (mut client, remote_prover) = init_client_and_prover().await;

            let fungible_asset = FungibleAsset::new(*FAUCET_ID, 10000000).unwrap();
            let (_, target_address) =
                Address::from_bech32("mdev1qr7zutlf2pnaayqjeajyuud7w3cqzrhqjwx")
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
