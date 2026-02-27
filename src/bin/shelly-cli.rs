//! Shelly CLI client
//!
//! A command-line client that communicates with the Shelly daemon via UDP.
//! Uses rustyline for readline-style editing and history.

use clap::Parser;
use rmp_serde::decode::Deserializer;
use rmp_serde::encode::Serializer;
use rustyline::Editor;
use rustyline::history::FileHistory;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::timeout;

/// Message types
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
enum MsgType {
    Request = 0x01,
    RequestAck = 0x02,
    Response = 0x03,
}

/// Request payload
#[derive(Debug, Serialize)]
struct RequestPayload {
    content: String,
}

/// Response payload
#[derive(Debug, Deserialize)]
struct ResponsePayload {
    content: String,
    is_error: bool,
}

/// CLI arguments
#[derive(Debug, Parser)]
#[command(name = "shelly-cli")]
#[command(about = "Shelly daemon CLI client")]
struct Args {
    /// Daemon address (e.g., 127.0.0.1:9700)
    #[arg(short, long, default_value = "127.0.0.1:9700")]
    target: SocketAddr,

    /// ACK timeout in seconds
    #[arg(long, default_value = "5")]
    timeout: u64,

    /// Maximum retry attempts
    #[arg(short, long, default_value = "3")]
    max_retries: u32,

    /// History file path
    #[arg(long)]
    history_file: Option<PathBuf>,

    /// Maximum history entries (reserved for future use)
    #[arg(long, default_value = "1000")]
    _history_size: usize,
}

/// CLI configuration
#[derive(Debug, Clone)]
struct Config {
    target: SocketAddr,
    ack_timeout_secs: u64,
    max_retries: u32,
    history_file: PathBuf,
    #[allow(dead_code)]
    history_size: usize,
}

impl Config {
    fn from_args(args: Args) -> Self {
        let history_file = args.history_file.unwrap_or_else(|| {
            dirs::home_dir()
                .map(|p| p.join(".shelly_history"))
                .unwrap_or_else(|| PathBuf::from(".shelly_history"))
        });

        Self {
            target: args.target,
            ack_timeout_secs: args.timeout,
            max_retries: args.max_retries,
            history_file,
            history_size: args._history_size,
        }
    }
}

/// Main client state
struct Client {
    socket: UdpSocket,
    config: Config,
    seq: AtomicU32,
}

impl Client {
    /// Create a new client
    async fn new(config: Config) -> io::Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;

        Ok(Self {
            socket,
            config,
            seq: AtomicU32::new(1),
        })
    }

    /// Send a request and wait for response
    async fn send_request(&self, content: String) -> io::Result<ResponsePayload> {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);

        // Serialize payload
        let payload = RequestPayload {
            content: content.clone(),
        };
        let mut payload_bytes = Vec::new();
        let mut ser = Serializer::new(&mut payload_bytes);
        payload
            .serialize(&mut ser)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        // Build packet: type (1) + seq (4) + payload
        let mut packet = vec![MsgType::Request as u8];
        packet.extend_from_slice(&seq.to_be_bytes());
        packet.extend_from_slice(&payload_bytes);

        // Send with retries
        for _attempt in 0..self.config.max_retries {
            // Send request
            self.socket.send_to(&packet, self.config.target).await?;

            // Wait for ACK
            match self.wait_for_ack(seq).await {
                Ok(true) => {
                    // Wait for response
                    match self.wait_for_response(seq).await {
                        Ok(response) => return Ok(response),
                        Err(_) => {
                            // Response timeout, retry
                            eprintln!("[warning] Response timeout, retrying...");
                            continue;
                        }
                    }
                }
                Ok(false) => continue, // Not our ACK, keep waiting
                Err(_) => continue,    // Timeout or error, retry
            }
        }

        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "shelly not responding",
        ))
    }

    /// Wait for REQUEST_ACK
    async fn wait_for_ack(&self, expected_seq: u32) -> io::Result<bool> {
        let mut buf = [0u8; 1024];

        match timeout(
            Duration::from_secs(self.config.ack_timeout_secs),
            self.socket.recv_from(&mut buf),
        )
        .await
        {
            Ok(Ok((len, addr))) => {
                if addr != self.config.target {
                    return Ok(false);
                }

                if len < 5 {
                    return Ok(false);
                }

                let msg_type = buf[0];
                let seq = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);

                if msg_type == MsgType::RequestAck as u8 && seq == expected_seq {
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Ok(false), // Timeout
        }
    }

    /// Wait for RESPONSE
    async fn wait_for_response(&self, expected_seq: u32) -> io::Result<ResponsePayload> {
        let mut buf = [0u8; 65536];

        // Longer timeout for response (inference may take time)
        match timeout(Duration::from_secs(120), self.socket.recv_from(&mut buf)).await {
            Ok(Ok((len, addr))) => {
                if addr != self.config.target {
                    return Err(io::Error::other("Unexpected sender"));
                }

                if len < 5 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Packet too short",
                    ));
                }

                let msg_type = buf[0];
                let seq = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);

                if msg_type != MsgType::Response as u8 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Not a response packet",
                    ));
                }

                if seq != expected_seq {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Sequence mismatch",
                    ));
                }

                // Deserialize payload
                let mut de = Deserializer::new(&buf[5..len]);
                let payload: ResponsePayload = Deserialize::deserialize(&mut de)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

                Ok(payload)
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(io::Error::new(io::ErrorKind::TimedOut, "Response timeout")),
        }
    }
}

fn main() -> io::Result<()> {
    // Parse arguments
    let args = Args::parse();
    let config = Config::from_args(args);

    // Check locale
    if let Ok(lang) = std::env::var("LANG")
        && !lang.to_lowercase().contains("utf-8")
        && !lang.to_lowercase().contains("utf8")
    {
        eprintln!(
            "[warning] Terminal locale is not UTF-8. Non-ASCII characters may not display correctly."
        );
    }

    // Build runtime for async network operations
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async { run_client(config).await })
}

async fn run_client(config: Config) -> io::Result<()> {
    // Initialize client
    let client = Client::new(config.clone()).await?;

    // Initialize rustyline with history
    let mut rl: Editor<(), FileHistory> = Editor::new().map_err(io::Error::other)?;

    // Load history from file
    if config.history_file.exists()
        && let Err(e) = rl.load_history(&config.history_file)
        && config.history_file.exists()
    {
        eprintln!("[warning] Failed to load history: {}", e);
    }

    // Set max history size - truncate history file on load if too large
    // rustyline doesn't have a direct resize method, we handle this by limiting during save

    // Print welcome message
    println!("shelly-cli v{}", env!("CARGO_PKG_VERSION"));
    println!("Target: {}", client.config.target);
    println!("Type your message and press Enter. Ctrl+D to quit.");
    println!();

    // Main loop using rustyline
    loop {
        // Read a line with rustyline
        let readline = rl.readline("> ");

        match readline {
            Ok(line) => {
                let input = line.trim();
                if input.is_empty() {
                    continue;
                }

                // Add to history (skip duplicates - rustyline handles this)
                let _ = rl.add_history_entry(input);

                // Send request
                print!("[waiting...]");
                io::stdout().flush()?;

                match client.send_request(input.to_string()).await {
                    Ok(response) => {
                        // Clear waiting message and print response
                        print!("\r");
                        if response.is_error {
                            println!("[error] {}", response.content);
                        } else {
                            println!("{}", response.content);
                        }
                    }
                    Err(e) => {
                        // Clear waiting message and print error
                        print!("\r");
                        println!("[error] {}", e);
                    }
                }
            }
            Err(rustyline::error::ReadlineError::Interrupted) => {
                // Ctrl+C - cancel current input, continue
                println!("^C");
                continue;
            }
            Err(rustyline::error::ReadlineError::Eof) => {
                // Ctrl+D - exit
                break;
            }
            Err(e) => {
                eprintln!("[error] Readline error: {}", e);
                break;
            }
        }
    }

    // Save history
    if let Err(e) = rl.save_history(&config.history_file) {
        eprintln!("[warning] Failed to save history: {}", e);
    }

    println!("\nGoodbye!");
    Ok(())
}
