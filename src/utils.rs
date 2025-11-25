use std::{sync::Arc, time::Instant};

use miden_client::{
    Client, RemoteTransactionProver,
    address::Address,
    builder::ClientBuilder,
    keystore::FilesystemKeyStore,
    note_transport::grpc::GrpcNoteTransportClient,
    rpc::{Endpoint, GrpcClient},
};
use miden_client_sqlite_store::SqliteStore;
use rand::rngs::StdRng;

const TX_PROVER_ENDPOINT: &str = "https://tx-prover.testnet.miden.io";
const NOTE_TRANSPORT_URL: &str = "https://transport.miden.io";

pub fn validate_address(bech32_string: &str) -> bool {
    match Address::decode(bech32_string) {
        Ok((_, _)) => true,
        Err(_) => false,
    }
}

pub async fn init_client_and_prover(
    client_db: &str,
    endpoint: Endpoint,
) -> Client<FilesystemKeyStore<StdRng>> {
    let timeout_ms = 10_000;
    let rpc_api = Arc::new(GrpcClient::new(&endpoint, timeout_ms));
    let sqlite_store = SqliteStore::new(client_db.into()).await.unwrap();

    let note_tranport =
        GrpcNoteTransportClient::connect(NOTE_TRANSPORT_URL.to_string(), timeout_ms)
            .await
            .unwrap();
    let remote_prover = Arc::new(RemoteTransactionProver::new(TX_PROVER_ENDPOINT.to_string()));
    let mut client = ClientBuilder::new()
        .store(Arc::new(sqlite_store))
        .rpc(rpc_api)
        .filesystem_keystore("./keystore")
        .in_debug_mode(true.into())
        .note_transport(Arc::new(note_tranport))
        .prover(remote_prover)
        .build()
        .await
        .expect("Failed to build client");

    let time = Instant::now();
    client.sync_state().await.expect("Failed to sync state");
    println!("State synced in {}s", time.elapsed().as_secs_f32());

    client
}
