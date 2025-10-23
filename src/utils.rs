use std::{sync::Arc, time::Instant};

use bech32::{Bech32m, primitives::decode::CheckedHrpstring};
use miden_client::{
    Client, RemoteTransactionProver,
    account::{AccountId, Address, AddressType},
    builder::ClientBuilder,
    keystore::FilesystemKeyStore,
    rpc::{Endpoint, TonicRpcClient},
};
use rand::rngs::StdRng;

const SERIALIZED_SIZE: usize = 15;
const TX_PROVER_ENDPOINT: &str = "https://tx-prover.testnet.miden.io";

/// Copy from earlier version of Miden Base
pub fn legacy_accountid_to_bech32(bech32_string: &str) -> Result<AccountId, String> {
    // We use CheckedHrpString with an explicit checksum algorithm so we don't allow the
    // `Bech32` or `NoChecksum` algorithms.
    let checked_string = CheckedHrpstring::new::<Bech32m>(bech32_string)
        .map_err(|source| format!("Failed to decode bech32 string: {source}"))?;

    let mut byte_iter = checked_string.byte_iter();
    // The length must be the serialized size of the account ID plus the address byte.
    if byte_iter.len() != SERIALIZED_SIZE + 1 {
        return Err(format!(
            "Invalid address length: expected {}, got {}",
            SERIALIZED_SIZE + 1,
            byte_iter.len()
        ));
    }

    let address_byte = byte_iter.next().expect("there should be at least one byte");
    if address_byte != AddressType::AccountId as u8 {
        return Err(format!(
            "Invalid address type byte: expected {}, got {}",
            AddressType::AccountId as u8,
            address_byte
        ));
    }

    // Every byte is guaranteed to be overwritten since we've checked the length of the
    // iterator.
    let mut id_bytes = [0_u8; 15];
    for (i, byte) in byte_iter.enumerate() {
        id_bytes[i] = byte;
    }

    let account_id = AccountId::try_from(id_bytes)
        .map_err(|source| format!("Failed to create AccountId from bytes: {source}"))?;

    Ok(account_id)
}

pub fn validate_address(bech32_string: &str) -> bool {
    match Address::from_bech32(bech32_string) {
        Ok((_, Address::AccountId(_))) => true,
        Ok((_, _)) => true,
        Err(_) => {
            // Try legacy format
            legacy_accountid_to_bech32(bech32_string).is_ok()
        }
    }
}

pub async fn init_client_and_prover(
    client_db: &str,
) -> (
    Client<FilesystemKeyStore<StdRng>>,
    Arc<RemoteTransactionProver>,
) {
    let endpoint = Endpoint::testnet();
    let timeout_ms = 10_000;
    let rpc_api = Arc::new(TonicRpcClient::new(&endpoint, timeout_ms));
    let mut client: Client<FilesystemKeyStore<StdRng>> = ClientBuilder::new()
        .rpc(rpc_api)
        .filesystem_keystore("./keystore")
        .in_debug_mode(true.into())
        .sqlite_store(client_db)
        .build()
        .await
        .expect("Failed to build client");

    let time = Instant::now();
    client.sync_state().await.expect("Failed to sync state");
    println!("State synced in {}s", time.elapsed().as_secs_f32());

    let remote_prover = Arc::new(RemoteTransactionProver::new(TX_PROVER_ENDPOINT.to_string()));
    (client, remote_prover)
}
