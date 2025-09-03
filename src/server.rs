use std::error::Error;

use axum::{Json, Router, extract::Path, http::StatusCode, routing::get};
use lazy_static::lazy_static;
use miden_client::{
    account::{AccountId, Address},
    asset::FungibleAsset,
    note::NoteType,
    transaction::TransactionRequestBuilder,
};
use rusqlite::Connection;
use serde::Serialize;
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};

use crate::{
    tx_worker::{
        self, Transaction, get_number_of_tx_for_address, get_transactions_by_account, get_tx_by_id,
        get_txs_in_last_hour, get_txs_latest,
    },
    utils::{init_client_and_prover, validate_address},
};

lazy_static! {
    pub static ref FAUCET_ID: AccountId =
        AccountId::from_hex("0x5dc6b6ad6612e82065d874b67ae0c6").unwrap();
}

pub const STATS_FILE: &str = "./tx_stats.txt";
const APP_DB: &str = "./app_db.sqlite3";

async fn mint(Path((address, amount)): Path<(String, u64)>) -> Result<Json<String>, StatusCode> {
    println!("request for minting {} to {}", amount, address);
    validate_address(&address)
        .then(|| ())
        .ok_or(StatusCode::BAD_REQUEST)?;
    // Move the blocking client operations to a separate thread pool
    let result = tokio::task::spawn_blocking(move || {
        tokio::runtime::Handle::current().block_on(async {
            let (mut client, remote_prover) = init_client_and_prover().await;
            let conn = Connection::open(APP_DB).expect("FAILED TO OPEN DB");

            let account = client
                .get_account(*FAUCET_ID)
                .await
                .expect("Cannot get account")
                .unwrap();

            let account_full = account.account();

            if account_full.account_type().is_faucet() {
                let fungible_asset = FungibleAsset::new(account.account().id(), amount).unwrap();
                let (_, target_address) =
                    Address::from_bech32(&address).expect("Invalid address format");
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
                    .new_transaction(account.account().id(), transaction_request)
                    .await
                    .expect("Failed to execute transaction");
                let digest = transaction_execution_result.executed_transaction().id();
                client
                    .submit_transaction_with_prover(transaction_execution_result, remote_prover)
                    .await
                    .expect("Failed to submit transaction");

                conn.execute(
                    "INSERT OR IGNORE INTO ACCOUNTS (wallet_address) VALUES (?1)",
                    (&address,),
                )
                .expect("Failed to build transaction");

                return Json(digest.to_hex());
            }
            Json("No faucet account found".to_string())
        })
    })
    .await;

    result.map_err(|err| {
        println!("{:?}", err);
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

async fn add_address_if_not_there(Path(address): Path<String>) -> Result<(), StatusCode> {
    // validate if address is actully a address
    validate_address(&address)
        .then(|| ())
        .ok_or(StatusCode::BAD_REQUEST)?;
    println!("request for adding account {}", address);
    let conn = Connection::open(APP_DB).expect("FAILED TO OPEN DB");
    let res = conn
        .execute(
            "
            INSERT OR IGNORE INTO ACCOUNTS (wallet_address) VALUES (?1)
        ",
            (&address,),
        )
        .map_err(|err| {
            println!("{:?}", err.to_string());
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    println!("Added account {} to DB, result: {}", address, res);

    Ok(())
}

async fn get_transaciton_by_id(Path(tx_id): Path<String>) -> Result<Json<Transaction>, StatusCode> {
    let conn = Connection::open(APP_DB).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let tx = get_tx_by_id(&conn, tx_id).map_err(|err| {
        println!("{}", err);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(tx))
}

#[derive(Serialize)]
struct Stats {
    total_transactions: u32,
    transactions_in_last_hour: u32,
    wallets_created: u32,
    notes_created: u32,
    faucet_request: u32,
}

pub fn handle_db_error(err: Box<dyn Error>) -> StatusCode {
    println!("{:?}", err);
    StatusCode::INTERNAL_SERVER_ERROR
}

async fn get_stats() -> Result<Json<Stats>, StatusCode> {
    let conn = Connection::open(APP_DB).map_err(|err| handle_db_error(Box::new(err)))?;

    let mut stmt = conn
        .prepare("SELECT wallet_address FROM ACCOUNTS")
        .map_err(|err| handle_db_error(Box::new(err)))?;
    let rows = stmt
        .query_map([], |row| row.get::<usize, String>(0))
        .map_err(|err| handle_db_error(Box::new(err)))?;
    let num_wallets = rows.count();
    let (total_txs, total_notes_created, total_faucet_requests) =
        std::fs::read_to_string(STATS_FILE)
            .ok()
            .and_then(|s| {
                let parts: Vec<&str> = s.trim().split(',').collect();
                if parts.len() == 3 {
                    Some((
                        parts[0].parse::<u32>().unwrap_or(0),
                        parts[1].parse::<u32>().unwrap_or(0),
                        parts[2].parse::<u32>().unwrap_or(0),
                    ))
                } else {
                    None
                }
            })
            .unwrap_or((0, 0, 0));
    let stats = Stats {
        notes_created: total_notes_created,
        total_transactions: total_txs as u32,
        transactions_in_last_hour: get_txs_in_last_hour(&conn).map_err(|err| {
            println!("{}", err);
            StatusCode::INTERNAL_SERVER_ERROR
        })?,
        wallets_created: num_wallets as u32,
        faucet_request: total_faucet_requests,
    };
    Ok(Json(stats))
}

async fn get_txs_latest_api() -> Result<Json<Vec<Transaction>>, String> {
    let conn = Connection::open(APP_DB).map_err(|err| format!("Failed to open DB: {}", err))?;
    let txs = get_txs_latest(&conn)
        .map_err(|err| format!("Failed to get latest transactions: {}", err))?;
    Ok(Json(txs))
}

#[derive(serde::Serialize, Debug)]
struct ChartData {
    pub total_tx: u32,
    pub date: String,
}

async fn get_chart_data() -> Result<Json<Vec<ChartData>>, StatusCode> {
    let conn = Connection::open(APP_DB).map_err(|err| handle_db_error(Box::new(err)))?;
    let mut stmt = conn.prepare(
        "
            SELECT strftime('%Y-%m-%d', datetime(timestamp, 'unixepoch')) AS transaction_day, COUNT(*) AS daily_count
            FROM TRANSACTIONS_DETAIL
            WHERE date(timestamp, 'unixepoch') >= date('now', '-30 days')
            GROUP BY transaction_day
            ORDER BY transaction_day ASC
        ",
    ).map_err(|err| handle_db_error(Box::new(err)) )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(ChartData {
                total_tx: row.get(1)?,
                date: row.get(0)?,
            })
        })
        .map_err(|err| handle_db_error(Box::new(err)))?;
    let mut chart_data = vec![];
    for row in rows {
        chart_data.push(row.map_err(|err| handle_db_error(Box::new(err)))?);
    }
    if chart_data.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(Json(chart_data))
}

async fn get_transactions_for_account(
    Path((address, page_number)): Path<(String, u32)>,
) -> Result<Json<Vec<Transaction>>, StatusCode> {
    let conn = Connection::open(APP_DB).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Address::from_bech32(&address).map_err(|_| StatusCode::BAD_REQUEST)?;
    let res = get_transactions_by_account(&conn, &address, page_number).map_err(|err| {
        println!("{}", err);
        return StatusCode::INTERNAL_SERVER_ERROR;
    })?;
    if res.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(Json(res))
}

async fn get_tx_count_for_account(Path(address): Path<String>) -> Result<Json<u32>, StatusCode> {
    let conn = Connection::open(APP_DB).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Address::from_bech32(&address).map_err(|_| StatusCode::BAD_REQUEST)?;
    let res = get_number_of_tx_for_address(&conn, &address).map_err(|err| {
        println!("{}", err);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(res))
}

pub async fn start_server() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();

    // Get CORS allowed origins from environment variable
    let cors_origins = std::env::var("CORS_ALLOWED_ORIGINS").unwrap_or_else(|_| "*".to_string());

    // Configure CORS layer
    let cors_layer = if cors_origins == "*" {
        // Allow all origins
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
    } else {
        // Parse specific origins from comma-separated string
        let origins: Vec<_> = cors_origins
            .split(',')
            .map(|s| s.trim().parse().expect("Invalid origin URL"))
            .collect();

        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods(Any)
            .allow_headers(Any)
    };

    if !std::path::Path::new(APP_DB).exists() {
        // Only create tables if the database file does not exist
        let conn = Connection::open(APP_DB)?;

        conn.execute(
            "
        CREATE TABLE ACCOUNTS (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            wallet_address TEXT NOT NULL UNIQUE
        )",
            (),
        )?;
        conn.execute(
            "
            CREATE TABLE TRANSACTIONS_DETAIL (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                block_num INTEGER NOT NULL,
                tx_id TEXT NOT NULL UNIQUE,
                tx_kind TEXT CHECK(tx_kind IN ('faucet_request', 'send', 'receive')) NOT NULL,
                sender TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                note_id TEXT NULL DEFAULT NULL,
                note_type TEXT NULL DEFAULT NULL,
                note_aux TEXT NULL DEFAULT NULl
            )
        ",
            (),
        )?;
    }

    let app = Router::new()
        .route("/mint/{address}/{amount}", get(mint))
        .route("/add/{address}", get(add_address_if_not_there))
        .route("/transaction/{tx_id}", get(get_transaciton_by_id))
        .route("/stats", get(get_stats))
        .route("/latest-transactions", get(get_txs_latest_api))
        .route("/chart-data", get(get_chart_data))
        .route(
            "/transactions/{address}/{page_number}",
            get(get_transactions_for_account),
        )
        .route(
            "/transactions/{address}/count",
            get(get_tx_count_for_account),
        )
        .layer(ServiceBuilder::new().layer(cors_layer));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();

    println!("Server starting on 0.0.0.0:8080");
    println!("CORS origins: {}", cors_origins);
    tokio::spawn(tx_worker::start_worker());
    axum::serve(listener, app).await?;

    Ok(())
}
