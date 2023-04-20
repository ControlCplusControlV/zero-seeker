use clap::Arg;
use ethers::types::{H160, U256};
use libsecp256k1::{PublicKey, SecretKey};
use rayon::prelude::*;
use rlp::RlpStream;
use sha3::{Digest, Keccak256, Sha3_256};
use std::error::Error;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tiny_keccak::{Hasher, Keccak};

use clap::Parser;
#[derive(Parser, Default, Debug)]
#[clap(name = "ZeroSeeker", about = "")]

pub struct Args {
    #[clap(short, long, help = "Set the entropy seed")]
    pub entropy_seed: String,
    #[clap(short, long, help = "Set the desired number of total zero bytes")]
    pub zero_bytes: u8,
    #[clap(short, long, help = "Set whether zero bytes are leading or total")]
    pub leading: bool,
}

fn main() {
    let args = Args::parse();
    if args.leading {
        println!(
            "Generating address with {} leading zero bytes...",
            args.zero_bytes
        );
    } else {
        println!(
            "Generating address with {} total zero bytes...",
            args.zero_bytes
        );
    }

    let lower_complexity: u8 = 2;

    // Run the search for the lower complexity value
    let start_time = Instant::now();
    mine_address_with_n_zero_bytes(&args.entropy_seed, lower_complexity, args.leading);
    let elapsed_time = start_time.elapsed();

    // Calculate the ratio between the probabilities for the lower and target complexity values
    let ratio = (1.0 / 256.0f64.powi(lower_complexity as i32))
        / (1.0 / 256.0f64.powi(args.zero_bytes as i32));

    // Estimate the time it would take to find a matching Ethereum address for the desired complexity value
    let estimated_time = elapsed_time.as_secs_f64() * ratio;

    let estimated_seconds = estimated_time as u64;
    let (days, hours, minutes, seconds) = {
        let (days, remainder) = (estimated_seconds / 86_400, estimated_seconds % 86_400);
        let (hours, remainder) = (remainder / 3_600, remainder % 3_600);
        let (minutes, seconds) = (remainder / 60, remainder % 60);
        (days, hours, minutes, seconds)
    };

    println!(
        "Estimated time to find an address with {} zero bytes: {} days, {} hours, {} minutes, and {} seconds",
        args.zero_bytes, days, hours, minutes, seconds
    );

    if let Some((private_key, contract_address)) =
        mine_address_with_n_zero_bytes(&args.entropy_seed, args.zero_bytes, args.leading)
    {
        let elapsed_time = start_time.elapsed();
        if args.leading {
            println!(
                "Found address with {} leading zero bytes in {} seconds: {:?}",
                args.zero_bytes,
                elapsed_time.as_secs(),
                contract_address
            );
        } else {
            println!(
                "Found address with {} zero bytes in {} seconds: {:?}",
                args.zero_bytes,
                elapsed_time.as_secs(),
                contract_address
            );
        }
        println!("Private key: 0x{private_key}");
    } else {
        println!("No matching address found.");
    }
}

fn mine_address_with_n_zero_bytes(
    entropy_seed: &str,
    zero_bytes: u8,
    leading: bool,
) -> Option<(String, H160)> {
    let num_threads: usize = num_cpus::get();
    let found = Arc::new(AtomicBool::new(false));

    let result: Option<(String, H160)> = (0..num_threads).into_par_iter().find_map_any(|_| {
        let nonce_step = num_threads as u32;
        let mut counter = 0u128;

        let mut private_key = hash_entropy_seed(entropy_seed, counter);
        let mut address_buf = H160::default(); // Allocate the address buffer once, outside the loop
        let mut contract_address_buf = H160::default(); // Allocate the contract address buffer once, outside the loop
        let mut zero_byte_count: u8 = 0;

        while zero_byte_count < zero_bytes && !found.load(Ordering::Relaxed) {
            counter += nonce_step as u128;
            private_key = hash_entropy_seed(entropy_seed, counter);
            address_from_private_key(&private_key, &mut address_buf).unwrap();
            contract_address_from_sender(&address_buf, &mut contract_address_buf);
            if leading {
                zero_byte_count = count_leading_zero_bytes(&contract_address_buf);
            } else {
                zero_byte_count = count_zero_bytes(&contract_address_buf);
            }

            if zero_byte_count >= zero_bytes {
                found.store(true, Ordering::Relaxed);
                return Some((private_key, contract_address_buf));
            }
        }

        None
    });

    result
}

fn hash_entropy_seed(seed: &str, counter: u128) -> String {
    // Hash the random string using SHA3-256
    let mut hasher = Sha3_256::new();
    hasher.update(seed.as_bytes());
    hasher.update(&counter.to_le_bytes()); // Add the counter to the input data
    let hash = hasher.finalize();

    // Return the hash as a hex-encoded string
    format!("{hash:064x}")
}

fn address_from_private_key(
    private_key: &str,
    address_buf: &mut H160,
) -> Result<(), Box<dyn Error>> {
    // Parse the private key string to bytes and create a SecretKey
    let private_key_bytes = hex::decode(private_key)?;
    let secret_key = SecretKey::parse_slice(&private_key_bytes)?;

    // Derive the PublicKey from the SecretKey
    let public_key = PublicKey::from_secret_key(&secret_key);

    // Serialize the compressed public key
    let serialized_pubkey = public_key.serialize();

    // Hash the public key using Keccak-256
    let mut hasher = Keccak::v256();
    let mut hash = [0u8; 32];
    hasher.update(&serialized_pubkey[1..]); // Skip the first byte, as it only indicates the format
    hasher.finalize(&mut hash);

    // Retrieve the Ethereum address from the hash
    address_buf.assign_from_slice(&hash[12..]);

    Ok(())
}

fn contract_address_from_sender(sender: &H160, contract_address_buf: &mut H160) {
    // RLP encode the sender address and a nonce of 0
    let mut rlp_stream = RlpStream::new_list(2);
    rlp_stream.append(&sender.as_bytes());
    rlp_stream.append(&0u64);

    let encoded = rlp_stream.out();

    // Hash the RLP encoded data using Keccak-256
    let mut hasher = Keccak256::new();
    hasher.update(encoded);
    let hash = hasher.finalize();

    // The last 20 bytes of the hash are the contract address
    contract_address_buf.assign_from_slice(&hash.as_slice()[12..])
}

fn count_zero_bytes(address: &H160) -> u8 {
    address.as_bytes().iter().filter(|&byte| *byte == 0).count() as u8
}

fn count_leading_zero_bytes(address: &H160) -> u8 {
    address
        .as_bytes()
        .iter()
        .take_while(|&byte| *byte == 0)
        .count() as u8
}
