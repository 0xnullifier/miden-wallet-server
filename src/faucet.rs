use std::sync::Arc;

use miden_client::{
    Client, Felt,
    account::{
        AccountBuilder, AccountStorageMode, AccountType,
        component::{AuthRpoFalcon512, BasicFungibleFaucet},
    },
    asset::TokenSymbol,
    auth::AuthSecretKey,
    builder::ClientBuilder,
    crypto::SecretKey,
    keystore::FilesystemKeyStore,
    rpc::{Endpoint, TonicRpcClient},
};
use rand::rngs::StdRng;
use rand_core::TryRngCore;

pub async fn create_new_faucet(endpoint: Endpoint) -> Result<(), Box<dyn std::error::Error>> {
    let timeout_ms = 10_000;
    let rpc_api = Arc::new(TonicRpcClient::new(&endpoint, timeout_ms));
    let mut client: Client<FilesystemKeyStore<StdRng>> = ClientBuilder::new()
        .rpc(rpc_api)
        .filesystem_keystore("./keystore")
        .in_debug_mode(true.into())
        .sqlite_store("./new.sqlite3")
        .build()
        .await?;
    let keystore = FilesystemKeyStore::new("./keystore".into())?;
    let mut init_seed = [0u8; 32];
    client.rng().try_fill_bytes(&mut init_seed)?;

    // Faucet parameters
    let symbol = TokenSymbol::new("MID").unwrap();
    let decimals = 8;
    let max_supply = Felt::new(1_000_000_000_000_000_000u64); // 100 million MID with 8 decimals

    // Generate key pair
    let key_pair = SecretKey::with_rng(client.rng());

    // Build the account
    let builder = AccountBuilder::new(init_seed)
        .account_type(AccountType::FungibleFaucet)
        .storage_mode(AccountStorageMode::Public)
        .with_auth_component(AuthRpoFalcon512::new(key_pair.public_key()))
        .with_component(BasicFungibleFaucet::new(symbol, decimals, max_supply).unwrap());

    let (faucet_account, seed) = builder.build().unwrap();

    // Add the faucet to the client
    client
        .add_account(&faucet_account, Some(seed), false)
        .await?;

    // Add the key pair to the keystore
    keystore
        .add_key(&AuthSecretKey::RpoFalcon512(key_pair))
        .unwrap();

    println!("Faucet account ID: {:?}", faucet_account.id().to_hex());

    Ok(())
}
