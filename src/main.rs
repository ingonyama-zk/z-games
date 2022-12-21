// Copyright (C) 2019-2022 Ingonyama
// This file is part of the snarkVM library.

// The snarkVM library is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// The snarkVM library is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with the snarkVM library. If not, see <https://www.gnu.org/licenses/>.

mod client_info;
mod consts;
mod prover_state;

use prover_state::*;

use rayon::ThreadPoolBuilder;
use std::{
    fs::{File, OpenOptions},
    io,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

use crossterm::tty::IsTty;
use log::{debug, error, info, warn};
use tokio_rayon::AsyncThreadPool;
use tracing_subscriber::filter::EnvFilter;

use snarkvm_console::{
    network::Testnet3,
    prelude::{Deserialize, Deserializer, Network},
};
use snarkvm_console_account::{Address, PrivateKey, ToBytes, Write};
use snarkvm_synthesizer::{CoinbasePuzzle, EpochChallenge, ProverSolution, PuzzleConfig};

use rand::{thread_rng, CryptoRng, Rng, RngCore};

use clap::Parser;

use async_channel::{unbounded, Receiver, Sender};
use futures::{future::FutureExt, lock::Mutex, SinkExt, StreamExt};
use tokio::sync::watch;

#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
pub struct Args {
    /// Optional comment to be sent to server
    #[arg(long)]
    caption: Option<String>,

    /// The number of puzzle solvers. Defaults to NUM_CPUS/4
    #[arg(long, default_value_t = num_cpus::get() / 4)]
    parallel_num: usize,

    /// The number of threads in each puzzle solver
    #[arg(long, default_value_t = 4, value_parser = clap::value_parser!(u16).range(1..))]
    threads_num: u16,
}

impl core::fmt::Display for Args {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Args")
            .field("caption", &self.caption)
            .field("parallel_num", &self.parallel_num)
            .field("threads_num", &self.threads_num)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Challenge<N: Network> {
    task_id: String,
    epoch_challenge: EpochChallenge<N>,
}

fn init_logging() {
    std::env::set_var("RUST_LOG", "info");

    let filter = EnvFilter::from_default_env()
        .add_directive("mio=off".parse().unwrap())
        .add_directive("tokio_util=off".parse().unwrap());

    let _ =
        tracing_subscriber::fmt().with_env_filter(filter).with_ansi(io::stdout().is_tty()).with_target(true).try_init();
}

type CoinbasePuzzleInst = CoinbasePuzzle<Testnet3>;

fn sample_inputs(
    degree: u32,
    rng: &mut (impl CryptoRng + RngCore),
) -> (EpochChallenge<Testnet3>, Address<Testnet3>, u64) {
    let epoch_challenge = sample_epoch_challenge(degree, rng);
    let (address, nonce) = sample_address_and_nonce(rng);
    (epoch_challenge, address, nonce)
}

fn sample_epoch_challenge(degree: u32, rng: &mut (impl CryptoRng + RngCore)) -> EpochChallenge<Testnet3> {
    EpochChallenge::new(rng.next_u32(), Default::default(), degree).unwrap()
}

fn sample_address_and_nonce(rng: &mut (impl CryptoRng + RngCore)) -> (Address<Testnet3>, u64) {
    let private_key = PrivateKey::new(rng).unwrap();
    let address = Address::try_from(private_key).unwrap();
    let nonce = rng.next_u64();
    (address, nonce)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logging();
    let args = Args::parse();
    info!("Started: {:#}", args);

    let c_info = client_info::ClientInfo::new(&args)?;
    info!("{:#?}", c_info);

    let rng = &mut thread_rng();

    let max_degree = 1 << 15;
    let max_config = PuzzleConfig { degree: max_degree };
    info!("Loading CoinbasePuzzle...");
    let universal_srs = CoinbasePuzzle::<Testnet3>::setup(max_config).unwrap();

    let degree = (1 << 13) - 1;
    let config = PuzzleConfig { degree };
    let coinbase_puzzle = CoinbasePuzzleInst::trim(&universal_srs, config).unwrap();
    let (epoch_challenge, address, _) = sample_inputs(degree, rng);

    let challenge = Challenge { task_id: "zk_games".to_string(), epoch_challenge: epoch_challenge.clone() };

    let results_file_path = format!("{}_{}", epoch_challenge.clone().epoch_number(), consts::RESULTS_FILE_TEMPLATE);
    let mut results_file = File::create(results_file_path).unwrap();
    results_file.write(format!("{:#?}", c_info).as_bytes());
    results_file.write_all(b"===coinbase_target===");
    results_file.write_all(&consts::SHARE_TARGET.to_bytes_le().unwrap());
    results_file.write_all(b"===proof_target===");
    results_file.write_all(&[0u8]); //
    results_file.write_all(b"===epoch_challenge===");
    results_file.write_all(&epoch_challenge.to_bytes_le().unwrap());
    results_file.write_all(b"===max_degree===");
    results_file.write_all(&max_degree.to_bytes_le().unwrap());
    results_file.write_all(b"===degree===");
    results_file.write_all(&degree.to_bytes_le().unwrap());
    results_file.write_all(b"===solutions===").unwrap();

    tokio::spawn(do_puzzle(args.clone(), coinbase_puzzle, challenge, address));
    Ok(())
}

async fn do_puzzle<N: Network>(
    args: Args,
    coinbase_puzzle: CoinbasePuzzle<N>,
    challenge: Challenge<N>,
    address: Address<N>,
) {
    let threads_num = args.threads_num;
    let parallel_num = args.parallel_num;

    let mut thread_pools = Vec::new();

    for _ in 0..parallel_num {
        let rayon_panic_handler = move |err: Box<dyn core::any::Any + Send>| {
            error!("{:?} - just skip", err);
        };
        thread_pools.push(Arc::new(
            ThreadPoolBuilder::new()
                .stack_size(consts::THREAD_STACK_SIZE)
                .num_threads(threads_num as usize)
                .panic_handler(rayon_panic_handler)
                .build()
                .expect("Failed to initialize a thread pool for worker using cpu"),
        ));
    }

    let mut provers_state = start_provers(thread_pools.clone(), coinbase_puzzle.clone(), challenge.clone(), address);

    loop {
        std::thread::sleep(Duration::from_millis(5000));

        let task_id = provers_state.task_id.clone();
        let elapsed = provers_state.proves_start.read().unwrap().elapsed().as_secs_f64();
        let iterations = provers_state.proves_count.load(Ordering::SeqCst) as usize;
        if elapsed >= 60f64 {
            info!(
                "iteration: {elapsed:.2} s (with {iterations} proves_total, {:.2} s/s hashrate)",
                iterations as f64 / (elapsed - 60f64)
            );
        } else {
            info!("warming up");
        }

        if elapsed >= 600f64 {
            provers_state.proves_count.fetch_add(1, Ordering::SeqCst);
            debug!("Terminating...");
            break;
        }
    }
}

fn start_provers<N: Network>(
    thread_pools: Vec<Arc<rayon::ThreadPool>>,
    coinbase_puzzle: CoinbasePuzzle<N>,
    challenge: Challenge<N>,
    address: Address<N>,
) -> ProverState {
    let coinbase_target = consts::SHARE_TARGET;

    let epoch_challenge = &challenge.epoch_challenge;
    let epoch_number = epoch_challenge.clone().epoch_number();
    let task_id = &challenge.task_id;
    info!("Received challenge.task_id: {task_id}");
    let provers_state = ProverState::new(task_id.clone());

    for i in 0..thread_pools.len() {
        let task_id = task_id.clone();
        let prover_coinbase_puzzle = coinbase_puzzle.clone();
        let prover_epoch_challenge = epoch_challenge.clone();
        let prover_challenge_rx = challenge.clone();

        let state = provers_state.clone();
        let task = thread_pools[i].spawn_async(move || {
            //let _gpu_device_id = i % *GPU_COUNT as usize;
            let mut rng = thread_rng();
            loop {
                let result = prover_coinbase_puzzle
                    .prove(&prover_epoch_challenge, address, rng.gen(), Some(coinbase_target))
                    .ok()
                    .and_then(|solution| solution.to_target().ok().map(|solution_target| (solution_target, solution)));

                let elapsed = state.proves_start.read().unwrap().elapsed().as_secs_f64();
                if elapsed >= 60f64 {
                    state.proves_count.fetch_add(1, Ordering::SeqCst);
                }

                // If the prover found a solution, then save it.
                if let Some((solution_target, solution)) = result {
                    info!("Found a Solution '{}' (Proof Target {solution_target})", solution.commitment());
                    let results_file_path = format!("{}_{}", epoch_number, consts::RESULTS_FILE_TEMPLATE);
                    let mut results_file = OpenOptions::new().write(true).append(true).open(results_file_path).unwrap();
                    results_file.write_all(&solution.to_bytes_le().unwrap()).unwrap();
                    results_file.write_all(b"===").unwrap();
                }

                if elapsed >= 600f64 {
                    state.proves_count.fetch_add(1, Ordering::SeqCst);
                    debug!("Terminating...");
                    break;
                }
            }
        });

        provers_state.pools.write().unwrap().push(task);
    }

    provers_state
}
