use std::num::NonZeroU32;
use std::process;

use tokio::io::AsyncWriteExt;
use tokio::net::{TcpStream, UdpSocket};

use clap::Parser;
use chrono::prelude::*;
use governor::{Quota, RateLimiter};
use hostname;

#[derive(Parser)]
struct Cli {
    /// network address of syslog server in format <ip>:<port>
    #[clap(short, long, value_parser)]
    addr: String,
    /// whether to use UDP instead of TCP
    #[clap(short = 'u', long = "udp", action)]
    use_udp: bool,
    /// whether to use 3164 (DBS) of 5424 syslog format
    #[clap(short = 'b', long = "bsd", action)]
    use_bsd: bool,
    /// Messages per second
    #[clap(short, long)]
    rate: Option<NonZeroU32>,
}

enum MsgType {
    Syslog3164,
    Syslog5424,
}

#[inline(always)]
fn get_syslog_msg(msg_type: &MsgType, pid: u32, msg_id: i32) -> String {
    let hostname = hostname::get().expect("Unable to get hostname!").into_string().expect("Unable to convert hostname to UTF-8, wtf is your hostname anyway?");
    let datetime = Local::now();
    match msg_type {
        MsgType::Syslog3164 => format!(
            "<34>{} {} spamlog[{}]: msg_id {} \n",
            datetime.naive_local().format("%b %e %H:%M:%S"), hostname, pid, msg_id
        ),
        MsgType::Syslog5424 => format!(
            "<34>1 {} {} spamlog {} {} - hello from rust!\n",
            datetime.to_rfc3339(), hostname, pid, msg_id
        ),
    }
}

async fn spam_tcp(addr: &str, msg_type: MsgType, rate: NonZeroU32) {
    let mut stream = TcpStream::connect(addr).await.expect("Unable to connect!");
    let limiter = RateLimiter::direct(Quota::per_second(rate));
    let pid = process::id();
    println!("Spamming {addr} over TCP");
    for num in 0.. {
        let msg = get_syslog_msg(&msg_type, pid, num);
        limiter.until_ready().await;
        stream.write(msg.as_bytes()).await.unwrap();
    }
}

async fn ddos_tcp(addr: &str, msg_type: MsgType) {
    let mut stream = TcpStream::connect(addr).await.expect("Unable to connect!");
    let pid = process::id();
    println!("DDoS'ing {addr} over TCP");
    for num in 0.. {
        let msg = get_syslog_msg(&msg_type, pid, num);
        stream.write(msg.as_bytes()).await.unwrap();
    }
}

async fn spam_udp(addr: &str, msg_type: MsgType, rate: NonZeroU32) {
    let socket = UdpSocket::bind("0.0.0.0:2345")
        .await
        .expect("Unable to bind!");
    let pid = process::id();

    let limiter = RateLimiter::direct(Quota::per_second(rate));
    println!("Spamming {addr} over UDP");
    for num in 0.. {
        let msg = get_syslog_msg(&msg_type, pid, num);
        limiter.until_ready().await;
        socket.send_to(msg.as_bytes(), addr).await.unwrap();
    }
}

async fn ddos_udp(addr: &str, msg_type: MsgType) {
    let socket = UdpSocket::bind("0.0.0.0:2345")
        .await
        .expect("Unable to bind!");
    let pid = process::id();
    println!("DDoS'ing {addr} over UDP");
    for num in 0.. {
        let msg = get_syslog_msg(&msg_type, pid, num);
        socket.send_to(msg.as_bytes(), addr).await.unwrap();
    }
}
#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let msg_type;
    if cli.use_bsd {
        msg_type = MsgType::Syslog3164;
    } else {
        msg_type = MsgType::Syslog5424;
    }
    if cli.use_udp {
        match cli.rate {
            Some(r) => spam_udp(&cli.addr, msg_type, r).await,
            None => ddos_udp(&cli.addr, msg_type).await,
        }
    } else {
        match cli.rate {
            Some(r) => spam_tcp(&cli.addr, msg_type, r).await,
            None => ddos_tcp(&cli.addr, msg_type).await,
        }
    }
}
