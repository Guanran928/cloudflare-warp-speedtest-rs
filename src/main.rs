mod args;

use crate::args::{Args, SpeedTestMode};
use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::generate;
use futures::StreamExt;
use hex::decode;
use indicatif::{ProgressBar, ProgressStyle};
use ipnetwork::Ipv4Network;
use log::{debug, info};
use rand::seq::{IndexedRandom, IteratorRandom};
use std::io;
use std::net::{SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Instant;
use tokio::net::UdpSocket;
use tokio::time::{Duration, timeout};

#[derive(Debug)]
struct TestResult {
    addr: SocketAddr,
    latency: u128,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .format_target(false)
        .format_timestamp(None)
        .parse_default_env()
        .init();

    let cli = Args::parse();

    // Generate shell completions
    if let Some(generator) = cli.completion {
        let mut cmd = Args::command();
        let name = cmd.get_name().to_string();
        generate(generator, &mut cmd, &name, &mut io::stdout());
        return Ok(());
    };

    let addrs = match cli.mode {
        SpeedTestMode::Ipv4 => generate_ipv4(cli.addresses),
        SpeedTestMode::Ipv6 => todo!(),
    };

    let progress_bar: Option<Arc<ProgressBar>> = if !log::log_enabled!(log::Level::Debug) {
        let pb = Arc::new(ProgressBar::new(addrs.len() as u64 * cli.attempts as u64));

        pb.set_style(
            ProgressStyle::default_bar()
                .template(
                    "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}",
                )
                .unwrap()
                .progress_chars("#>-"),
        );

        pb.set_message(format!(
            "({} addresses * {} attempts)",
            addrs.len(),
            cli.attempts
        ));

        Some(pb)
    } else {
        None
    };

    let stream = tokio_stream::iter(addrs.into_iter())
        .map(|ip_port| {
            let pb: Option<Arc<ProgressBar>> = progress_bar.as_ref().map(Arc::clone);
            async move {
                let mut latencies = Vec::with_capacity(cli.attempts as usize);
                for _ in 0..cli.attempts {
                    if let Some(pb) = pb.as_ref() {
                        pb.inc(1);
                    }
                    if let Ok(result) = speedtest(&ip_port).await {
                        latencies.push(result.latency);
                    }
                }

                if latencies.is_empty() {
                    None
                } else {
                    let avg_latency = latencies.iter().sum::<u128>() / latencies.len() as u128;
                    Some(TestResult {
                        addr: ip_port,
                        latency: avg_latency,
                    })
                }
            }
        })
        .buffer_unordered(cli.threads)
        .filter_map(|res| async move { res });

    let mut alive_addrs: Vec<TestResult> = stream.collect().await;
    alive_addrs.sort_by_key(|res| res.latency);

    if let Some(pb) = progress_bar {
        pb.finish_with_message("Done!");
    }

    info!(
        "Found {} working IPs out of {} IPs",
        alive_addrs.len(),
        cli.addresses
    );

    info!("Top 5 IPs with lowest latency:");
    for result in alive_addrs.iter().take(5) {
        info!("{} - {} ms", result.addr, result.latency);
    }

    Ok(())
}

/// Generate `amount` of random IPv4 addresses with a random port.
fn generate_ipv4(amount: usize) -> Vec<SocketAddr> {
    let v4_ranges = [
        "162.159.192.0/24",
        "162.159.193.0/24",
        "162.159.195.0/24",
        "162.159.204.0/24",
        "188.114.96.0/24",
        "188.114.97.0/24",
        "188.114.98.0/24",
        "188.114.99.0/24",
    ];

    let ports = [
        500, 854, 859, 864, 878, 880, 890, 891, 894, 903, 908, 928, 934, 939, 942, 943, 945, 946,
        955, 968, 987, 988, 1002, 1010, 1014, 1018, 1070, 1074, 1180, 1387, 1701, 2408, 4500, 5050,
        5242, 6515, 7103, 7152, 7156, 7281, 7559, 8319, 8742, 8854, 8886,
    ];

    let mut rng = rand::rng();

    let all_ips: Vec<_> = v4_ranges
        .iter()
        .flat_map(|cidr| {
            let network: Ipv4Network = cidr.parse().expect("Invalid CIDR");
            network.iter()
        })
        .collect();

    all_ips
        .iter()
        .choose_multiple(&mut rng, amount)
        .iter()
        .map(|&addr| {
            let port = ports.choose(&mut rng).unwrap();
            SocketAddr::V4(SocketAddrV4::new(*addr, *port))
        })
        .collect()
}

/// Measures the latency to a Cloudflare Warp node through UDP
async fn speedtest(addr: &SocketAddr) -> Result<TestResult> {
    let warp_handshake_packet = "013cbdafb4135cac96a29484d7a0175ab152dd3e59be35049beadf758b8d48af14ca65f25a168934746fe8bc8867b1c17113d71c0fac5c141ef9f35783ffa5357c9871f4a006662b83ad71245a862495376a5fe3b4f2e1f06974d748416670e5f9b086297f652e6dfbf742fbfc63c3d8aeb175a3e9b7582fbc67c77577e4c0b32b05f92900000000000000000000000000000000";
    let packet_data = decode(warp_handshake_packet).expect("Invalid hex string");

    let bind_addr = match addr {
        SocketAddr::V4(_) => "0.0.0.0:0",
        SocketAddr::V6(_) => "[::]:0",
    };
    let socket = UdpSocket::bind(bind_addr).await?;

    socket.send_to(&packet_data, addr).await?;
    debug!("Pinging {addr}");

    let mut buf = [0u8; 92];
    let start = Instant::now();

    let recv_result = timeout(Duration::from_secs(1), socket.recv_from(&mut buf)).await;

    match recv_result {
        Ok(Ok((len, src))) => {
            let elapsed = start.elapsed().as_millis();
            debug!("Received {len} bytes from {src} in {elapsed} ms");

            Ok(TestResult {
                addr: *addr,
                latency: elapsed,
            })
        }
        Ok(Err(e)) => {
            // Underlying recv_from error
            Err(e.into())
        }
        Err(_) => {
            // Timeout elapsed
            debug!("Timeout from {addr}");
            Err(anyhow::anyhow!(
                "Timeout waiting for response from {}",
                addr
            ))
        }
    }
}
