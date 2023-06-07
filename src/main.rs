use std::fs::File;
use std::io::Read;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::process;

use tokio::io::AsyncWriteExt;
use tokio::net::{TcpStream, UdpSocket};

use chrono::prelude::*;
use clap::{Parser, ValueEnum};
use governor::{Quota, RateLimiter};

#[derive(Parser)]
#[command(about = "Simple syslog spammer for load testing syslog servers. By default uses Syslog 5414 over TCP", long_about = None)]
struct Cli {
    /// message body to use. Can not be used together with --file
    #[arg(default_value = "hello from rust!", conflicts_with = "file")]
    body: String,
    /// path to a file with message body to use
    #[arg(short, long)]
    file: Option<PathBuf>,
    /// network address of syslog server in format <ip>:<port>
    #[arg(short, long, value_parser)]
    addr: String,
    /// what trasport layer protocol to use
    #[arg(short, long, value_enum,  default_value_t = Transport::TCP)]
    transport: Transport,
    /// what syslog protocol to use
    #[arg(short, long, value_enum, default_value_t = Protocol::Syslog5424)]
    portocol: Protocol,
    /// Messages per second
    #[arg(short, long)]
    rate: Option<NonZeroU32>,
}

#[derive(Clone, ValueEnum)]
enum Protocol {
    Syslog3164,
    Syslog5424,
}

#[derive(Clone, ValueEnum)]
enum Transport {
    TCP,
    UDP,
}

#[inline(always)]
fn get_syslog_msg(msg_type: &Protocol, pid: u32, msg_id: i32, body: &str) -> String {
    let hostname = hostname::get()
        .expect("Unable to get hostname!")
        .into_string()
        .expect("Unable to convert hostname to UTF-8, wtf is your hostname anyway?");
    let datetime = Local::now();
    match msg_type {
        Protocol::Syslog3164 => format!(
            "<34>{} {} spamlog[{}]: msg_id {} {}\n",
            datetime.naive_local().format("%b %e %H:%M:%S"),
            hostname,
            pid,
            msg_id,
            body,
        ),
        Protocol::Syslog5424 => format!(
            "<34>1 {} {} spamlog {} {} - {}\n",
            datetime.to_rfc3339_opts(SecondsFormat::Millis, false),
            hostname,
            pid,
            msg_id,
            body,
        ),
    }
}

async fn spam_tcp(addr: &str, msg_type: Protocol, rate: NonZeroU32, body: &str) {
    let mut stream = TcpStream::connect(addr).await.expect("Unable to connect!");
    let limiter = RateLimiter::direct(Quota::per_second(rate));
    let pid = process::id();
    println!("Spamming {addr} over TCP");
    for num in 0.. {
        let msg = get_syslog_msg(&msg_type, pid, num, body);
        limiter.until_ready().await;
        stream.write_all(msg.as_bytes()).await.unwrap();
    }
}

async fn ddos_tcp(addr: &str, msg_type: Protocol, body: &str) {
    let mut stream = TcpStream::connect(addr).await.expect("Unable to connect!");
    let pid = process::id();
    println!("DDoS'ing {addr} over TCP");
    for num in 0.. {
        let msg = get_syslog_msg(&msg_type, pid, num, body);
        stream.write_all(msg.as_bytes()).await.unwrap();
    }
}

async fn spam_udp(addr: &str, msg_type: Protocol, rate: NonZeroU32, body: &str) {
    let socket = UdpSocket::bind("0.0.0.0:2345")
        .await
        .expect("Unable to bind!");
    let pid = process::id();

    let limiter = RateLimiter::direct(Quota::per_second(rate));
    println!("Spamming {addr} over UDP");
    for num in 0.. {
        let msg = get_syslog_msg(&msg_type, pid, num, body);
        limiter.until_ready().await;
        socket.send_to(msg.as_bytes(), addr).await.unwrap();
    }
}

async fn ddos_udp(addr: &str, msg_type: Protocol, body: &str) {
    let socket = UdpSocket::bind("0.0.0.0:2345")
        .await
        .expect("Unable to bind!");
    let pid = process::id();
    println!("DDoS'ing {addr} over UDP");
    for num in 0.. {
        let msg = get_syslog_msg(&msg_type, pid, num, body);
        socket.send_to(msg.as_bytes(), addr).await.unwrap();
    }
}
#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let body = cli
        .file
        .and_then(|x| {
            let mut file = File::open(x).unwrap();
            let mut data = String::new();
            let bytes = file.read_to_string(&mut data).unwrap();
            if bytes != 0 {
                Some(data)
            } else {
                None
            }
        })
        .unwrap_or(cli.body);
    match cli.transport {
        Transport::UDP => match cli.rate {
            Some(r) => spam_udp(&cli.addr, cli.portocol, r, &body).await,
            None => ddos_udp(&cli.addr, cli.portocol, &body).await,
        },
        Transport::TCP => match cli.rate {
            Some(r) => spam_tcp(&cli.addr, cli.portocol, r, &body).await,
            None => ddos_tcp(&cli.addr, cli.portocol, &body).await,
        },
    }
}
