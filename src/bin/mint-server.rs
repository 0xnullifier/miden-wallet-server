use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use miden_client::account::{AccountId, Address};
use miden_client::asset::FungibleAsset;
use miden_client::note::{NoteType, create_p2id_note};
use miden_client::transaction::{OutputNote, TransactionRequestBuilder};
use miden_client::{ClientError, Felt};
use miden_faucet_server::utils::init_client_and_prover;
use threadpool::ThreadPool;
use tokio::runtime::Builder;
use tokio::sync::oneshot;

use lazy_static::lazy_static;

lazy_static! {
    pub static ref FAUCET_ID: AccountId =
        AccountId::from_hex(&std::env::var("FAUCET_ID").unwrap()).unwrap();
}

#[derive(Debug)]
struct MintRequest {
    address: String,
    amount: u64,
    response_tx: oneshot::Sender<Result<String, String>>,
}

type MintQueue = Arc<Mutex<VecDeque<MintRequest>>>;

lazy_static! {
    static ref MINT_QUEUE: MintQueue = Arc::new(Mutex::new(VecDeque::new()));
}

async fn bulk_mint(requests: &[(String, u64)]) -> Result<String, String> {
    let (mut client, prover) = init_client_and_prover().await;
    let mut p2id_notes = Vec::new();
    for (address, amount) in requests {
        let fungible_asset = FungibleAsset::new(*FAUCET_ID, *amount).unwrap();
        let target = match Address::from_bech32(address) {
            Ok((_, addr)) => match addr {
                Address::AccountId(aid) => aid.id(),
                _ => continue,
            },
            Err(_) => continue,
        };
        let p2id_note = create_p2id_note(
            *FAUCET_ID,
            target,
            vec![fungible_asset.into()],
            NoteType::Public,
            Felt::new(0),
            client.rng(),
        )
        .map_err(|e| e.to_string())?;
        p2id_notes.push(p2id_note);
    }
    let output_notes: Vec<OutputNote> = p2id_notes.into_iter().map(OutputNote::Full).collect();
    let transaction_request = TransactionRequestBuilder::new()
        .own_output_notes(output_notes)
        .build()
        .unwrap();
    let tx_execution_result = client
        .new_transaction(*FAUCET_ID, transaction_request)
        .await?;
    let digest = tx_execution_result.executed_transaction().id().to_hex();

    match client
        .submit_transaction_with_prover(tx_execution_result.clone(), prover)
        .await
    {
        Ok(_) => Ok(digest),
        Err(e) => match e {
            ClientError::TransactionProvingError(_) => {
                let time = Instant::now();
                println!("Got proving error, {:?}", e);
                client
                    .submit_transaction(tx_execution_result)
                    .await
                    .map_err(|e| e.to_string())?;
                println!("locally proven in {:?}", time.elapsed());
                Ok(digest)
            }
            _ => Err(e.to_string()),
        },
    }
}

fn start_queue_processor() -> JoinHandle<()> {
    let queue = MINT_QUEUE.clone();

    thread::spawn(move || {
        let rt = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to build Tokio runtime");

        loop {
            thread::sleep(Duration::from_secs(10));

            let mut pending_requests = Vec::new();

            {
                let mut queue_lock = queue.lock().unwrap();
                while let Some(request) = queue_lock.pop_front() {
                    pending_requests.push(request);
                }
            }

            if pending_requests.is_empty() {
                println!("No pending requests to process");
                continue;
            }

            println!("Processing batch of {} requests", pending_requests.len());

            let mint_data: Vec<(String, u64)> = pending_requests
                .iter()
                .map(|req| (req.address.clone(), req.amount))
                .collect();

            let result = rt.block_on(bulk_mint(&mint_data));

            for request in pending_requests {
                let _ = request.response_tx.send(result.clone());
            }
        }
    })
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
                    println!(
                        "Queuing mint request: {} tokens to address {}",
                        amount, address
                    );

                    let (tx, rx) = oneshot::channel();

                    let mint_request = MintRequest {
                        address: address.to_string(),
                        amount,
                        response_tx: tx,
                    };

                    {
                        let mut queue = MINT_QUEUE.lock().unwrap();
                        queue.push_back(mint_request);
                    }

                    let res = rt.block_on(async {
                        match tokio::time::timeout(Duration::from_secs(120), rx).await {
                            Ok(Ok(result)) => result,
                            Ok(Err(_)) => Err("Channel closed".to_string()),
                            Err(_) => Err("Timeout waiting for batch processing".to_string()),
                        }
                    });

                    let response = match res {
                        Ok(digest) => format!(
                            "HTTP/1.1 200 OK\r\n\
Content-Type: text/plain\r\n\
Access-Control-Allow-Origin: *\r\n\
Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
Access-Control-Allow-Headers: Content-Type\r\n\
\r\n{}",
                            digest
                        ),
                        Err(error) => format!(
                            "HTTP/1.1 500 INTERNAL SERVER ERROR\r\n\
Content-Type: text/plain\r\n\
Access-Control-Allow-Origin: *\r\n\
Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
Access-Control-Allow-Headers: Content-Type\r\n\
\r\nError: {}",
                            error
                        ),
                    };
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
    let n_workers = 20;
    let pool = ThreadPool::new(n_workers);

    // Start the fucking queue processor!
    start_queue_processor();
    println!("Queue processor started - will process batches every 10 seconds");

    let listener = TcpListener::bind("127.0.0.1:9090")?;
    println!("Server running on http://127.0.0.1:9090");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                pool.execute(|| {
                    handle_client(stream);
                });
            }
            Err(e) => {
                eprintln!("Connection failed: {}", e);
            }
        }
    }

    Ok(())
}
