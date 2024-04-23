use std::fmt::Display;
use std::fs::File;
use std::io::Read;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::process;

use anyhow::{anyhow, bail, Context, Result};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpStream, UdpSocket};

use chrono::prelude::*;
use clap::{Parser, ValueEnum};
use governor::{Quota, RateLimiter};

#[derive(Parser)]
#[command(about = "Simple syslog spammer for load testing syslog servers. By default uses Syslog 5424 over TCP", long_about = None)]
struct Cli {
    /// message body to use. Can not be used together with --file
    #[arg(default_value = "hello from rust!", conflicts_with = "file")]
    body: String,
    /// path to a file with message body to use
    #[arg(short, long)]
    file: Option<PathBuf>,
    /// network addresss of syslog server in format <ip>:<port>
    #[arg(short, long, value_parser)]
    addr: String,
    /// what trasport layer protocol to use
    #[arg(short, long, value_enum,  default_value_t = Transport::Tcp)]
    transport: Transport,
    /// what syslog protocol to use
    #[arg(short, long, value_enum, default_value_t = Protocol::Syslog5424)]
    protocol: Protocol,
    /// Messages per second
    #[arg(short, long)]
    rate: Option<NonZeroU32>,
}

#[derive(Clone, ValueEnum)]
enum Protocol {
    Syslog3164,
    Syslog5424,
}

impl Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Protocol::Syslog3164 => "Syslog 3164",
                Protocol::Syslog5424 => "Syslog 5424",
            }
        )
    }
}

#[derive(Clone, ValueEnum)]
enum Transport {
    Tcp,
    Udp,
}

#[inline(always)]
fn get_syslog_msg(msg_type: &Protocol, pid: u32, msg_id: i32, body: &str) -> Result<String> {
    let hostname = hostname::get()
        .context("Unable to get hostname!")?
        .into_string()
        .map_err(|_| anyhow!("Unable to parse hostname to UTF-8"))?;
    let datetime = Local::now();
    let msg = match msg_type {
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
    };
    Ok(msg)
}

async fn spam_tcp(addr: &str, msg_type: Protocol, rate: NonZeroU32, body: &str) -> Result<()> {
    let mut stream = TcpStream::connect(addr)
        .await
        .context("Unable to connect to specified address")?;
    let limiter = RateLimiter::direct(Quota::per_second(rate));
    let pid = process::id();
    println!("Spamming {addr} over TCP {msg_type}");
    for num in 0.. {
        let msg = get_syslog_msg(&msg_type, pid, num, body)?;
        limiter.until_ready().await;
        stream
            .write_all(msg.as_bytes())
            .await
            .context("Unable to send the message!")?;
    }
    Ok(())
}

async fn ddos_tcp(addr: &str, msg_type: Protocol, body: &str) -> Result<()> {
    let mut stream = TcpStream::connect(addr)
        .await
        .context("Unable to connect to specified addresss")?;
    let pid = process::id();
    println!("DDoS'ing {addr} over TCP {msg_type}");
    for num in 0.. {
        let msg = get_syslog_msg(&msg_type, pid, num, body)?;
        stream
            .write_all(msg.as_bytes())
            .await
            .context("Unable to send the message!")?;
    }
    Ok(())
}

async fn spam_udp(addr: &str, msg_type: Protocol, rate: NonZeroU32, body: &str) -> Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:0")
        .await
        .context("Unable to bind the socket")?;
    let pid = process::id();

    let limiter = RateLimiter::direct(Quota::per_second(rate));
    println!("Spamming {addr} over UDP {msg_type}");
    for num in 0.. {
        let msg = get_syslog_msg(&msg_type, pid, num, body)?;
        limiter.until_ready().await;
        socket
            .send_to(msg.as_bytes(), addr)
            .await
            .context("Unable to send the message!")?;
    }
    Ok(())
}

async fn ddos_udp(addr: &str, msg_type: Protocol, body: &str) -> Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:0")
        .await
        .context("Unable to bind the socket")?;
    let pid = process::id();
    println!("DDoS'ing {addr} over UDP with {msg_type}");
    for num in 0.. {
        let msg = get_syslog_msg(&msg_type, pid, num, body)?;
        socket
            .send_to(msg.as_bytes(), addr)
            .await
            .context("Unable to send the message!")?;
    }
    Ok(())
}
#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let body = cli
        .file
        .map(|x| {
            let mut file = File::open(x).context("Unable to open file!")?;
            let mut data = String::new();
            let bytes = file
                .read_to_string(&mut data)
                .context("Unable to read file contents!")?;
            if bytes != 0 {
                Ok(data)
            } else {
                bail!("File is empty!")
            }
        })
        .unwrap_or(Ok(cli.body))?;
    match cli.transport {
        Transport::Udp => match cli.rate {
            Some(r) => spam_udp(&cli.addr, cli.protocol, r, &body).await?,
            None => ddos_udp(&cli.addr, cli.protocol, &body).await?,
        },
        Transport::Tcp => match cli.rate {
            Some(r) => spam_tcp(&cli.addr, cli.protocol, r, &body).await?,
            None => ddos_tcp(&cli.addr, cli.protocol, &body).await?,
        },
    }
    Ok(())
}
