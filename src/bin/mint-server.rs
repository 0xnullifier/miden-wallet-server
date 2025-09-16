use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

use miden_client::account::{AccountId, Address};
use miden_client::asset::FungibleAsset;
use miden_client::note::NoteType;
use miden_client::transaction::TransactionRequestBuilder;
use miden_faucet_server::server::APP_DB;
use miden_faucet_server::utils::init_client_and_prover;
use rusqlite::Connection;
use tokio::runtime::Builder;

use lazy_static::lazy_static;

lazy_static! {
    pub static ref FAUCET_ID: AccountId =
        AccountId::from_hex(&std::env::var("FAUCET_ID").unwrap()).unwrap();
}

fn handle_client(mut stream: TcpStream) {
    let mut buffer = [0; 1024];

    // Read incoming request
    match stream.read(&mut buffer) {
        Ok(_) => {
            // Print the request
            let request = String::from_utf8_lossy(&buffer);
            let request_path = request.lines().next().unwrap_or("");
            let rt = Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to build Tokio runtime");

            if request.starts_with("GET /mint/") {
                let params = request_path.split(" ").nth(1).unwrap_or("");
                let parts: Vec<&str> = params.trim_start_matches("/mint/").split('/').collect();
                if parts.len() == 2 {
                    let address = parts[0];
                    let amount = parts[1];

                    let amount: u64 = match amount.parse() {
                        Ok(a) => a,
                        Err(_) => {
                            let response = "HTTP/1.1 400 BAD REQUEST\r\nContent-Type: text/plain\r\n\r\nInvalid amount format.";
                            stream.write_all(response.as_bytes()).unwrap();
                            return;
                        }
                    };
                    println!("Minting {} tokens to address {}", amount, address);
                    let res = rt.block_on(async {
                        let (mut client, remote_prover) = init_client_and_prover().await;

                        let conn = Connection::open(APP_DB).expect("Failed to get DB connection");

                        let fungible_asset = FungibleAsset::new(*FAUCET_ID, amount).unwrap();
                        let (_, target_address) =
                            Address::from_bech32(&address).expect("Invalid address format");

                        let target_id = match target_address {
                            Address::AccountId(id) => id.id(),
                            _ => panic!("Target address is not an AccountId"),
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
                            .submit_transaction_with_prover(
                                transaction_execution_result,
                                remote_prover,
                            )
                            .await
                            .expect("Failed to submit transaction");

                        conn.execute(
                            "INSERT OR IGNORE INTO ACCOUNTS (wallet_address) VALUES (?1)",
                            (&address,),
                        )
                        .expect("Failed to insert wallet address");

                        digest.to_hex()
                    });
                    let response = format!(
                        "HTTP/1.1 200 OK\r\n\
Content-Type: text/plain\r\n\
Access-Control-Allow-Origin: *\r\n\
Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
Access-Control-Allow-Headers: Content-Type\r\n\
\r\n {}",
                        res
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                    return;
                } else {
                    let response = "HTTP/1.1 400 BAD REQUEST\r\nContent-Type: text/plain\r\n\r\nInvalid mint request format. Use /mint/<address>/<amount>";
                    stream.write_all(response.as_bytes()).unwrap();
                    return;
                }
            } else {
                let response =
                    "HTTP/1.1 404 NOT FOUND\r\nContent-Type: text/plain\r\n\r\nNot Found";
                stream.write_all(response.as_bytes()).unwrap();
                return;
            }
        }
        Err(e) => {
            eprintln!("Failed to read from client: {}", e);
        }
    }
}

/// NEED TO ADD THIS TO CREATE CONTEXT FOR THE ASYNC RUNTIME IN THE CLIENT
pub fn main() -> std::io::Result<()> {
    dotenvy::dotenv().ok();

    let listener = TcpListener::bind("127.0.0.1:9090")?;
    println!("Server running on http://127.0.0.1:9090");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                handle_client(stream);
            }
            Err(e) => {
                eprintln!("Connection failed: {}", e);
            }
        }
    }

    Ok(())
}
