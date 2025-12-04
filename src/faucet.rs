use lazy_static::lazy_static;
use miden_client::{
    Felt,
    account::{
        AccountBuilder, AccountStorageMode, AccountType,
        component::{AuthRpoFalcon512, BasicFungibleFaucet},
    },
    asset::TokenSymbol,
    auth::AuthSecretKey,
    crypto::rpo_falcon512::SecretKey,
    keystore::FilesystemKeyStore,
    rpc::Endpoint,
};
use rand_core::TryRngCore;

use crate::utils::init_client;

lazy_static! {
    pub static ref CLIENT_DB: String = std::env::var("CLIENT_DB").unwrap();
}

pub async fn create_new_faucet(endpoint: Endpoint) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = init_client(&CLIENT_DB, endpoint).await;
    let keystore = FilesystemKeyStore::new("./keystore".into())?;
    let mut init_seed = [0u8; 32];
    client.rng().try_fill_bytes(&mut init_seed)?;

    // Faucet parameters
    let symbol = TokenSymbol::new("MDN").unwrap();
    let decimals = 8;
    let max_supply = Felt::new(1_000_000_000_000_000_000u64); // 100 million MID with 8 decimals

    // Generate key pair
    let key_pair = SecretKey::with_rng(client.rng());

    // Build the account
    let builder = AccountBuilder::new(init_seed)
        .account_type(AccountType::FungibleFaucet)
        .storage_mode(AccountStorageMode::Public)
        .with_auth_component(AuthRpoFalcon512::new(key_pair.public_key().into()))
        .with_component(BasicFungibleFaucet::new(symbol, decimals, max_supply).unwrap());
    let faucet_account = builder.build().unwrap();

    // Add the faucet to the client
    client.add_account(&faucet_account, false).await?;

    // Add the key pair to the keystore
    keystore
        .add_key(&AuthSecretKey::RpoFalcon512(key_pair))
        .unwrap();

    println!("Faucet account ID: {:?}", faucet_account.id().to_hex());

    Ok(())
}
