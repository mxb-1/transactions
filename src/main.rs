use crate::engine::Transaction;
use crate::engine::TransactionEngine;
use std::env;

mod engine;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        panic!("Expected only 1 argument representing the input path")
    }
    let file_path_arg = &args[1];
    let mut rdr = csv::Reader::from_path(file_path_arg).expect("Could not read from path");
    let deserialized_records = rdr.deserialize::<Transaction>();
    let mut engine = TransactionEngine::new();
    for tx_res in deserialized_records {
        let tx = tx_res.expect("Failed to deserialize record");
        engine
            .process_transaction(tx)
            .expect("Failed to process transaction");
    }
    // Print the CSV header
    println!("client,available,held,total,locked");
    let accounts = engine.retrieve_accounts();
    // Print all the account records in CSV format via their `Display` impl
    for account in accounts {
        println!("{}", account);
    }
}
