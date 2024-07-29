use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use nix::pty::{openpty, Winsize};
use nix::unistd::{fork, setsid, ForkResult};
use std::os::unix::io::AsRawFd;
use std::process::Command;
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::protocol::Message;

#[tokio::main]
async fn main() -> Result<()> {
    let addr = "127.0.0.1:8081";
    let listener = TcpListener::bind(&addr).await?;

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(handle_connection(stream));
    }

    Ok(())
}

async fn handle_connection(stream: tokio::net::TcpStream) -> Result<()> {
    let ws_stream = accept_async(stream).await?;
    let (mut write, mut read) = ws_stream.split();

    let pty = openpty(None, None)?;
    match unsafe { fork() }? {
        ForkResult::Parent { child } => {
            tokio::spawn(async move {
                let mut buf = [0u8; 1024];
                loop {
                    match nix::unistd::read(pty.master, &mut buf) {
                        Ok(n) if n > 0 => {
                            let output = String::from_utf8_lossy(&buf[..n]).to_string();
                            if write.send(Message::Text(output)).await.is_err() {
                                break;
                            }
                        }
                        _ => break,
                    }
                }
            });

            while let Some(Ok(msg)) = read.next().await {
                if let Message::Text(text) = msg {
                    nix::unistd::write(pty.master, text.as_bytes())?;
                }
            }
        }
        ForkResult::Child => {
            setsid()?;
            nix::unistd::dup2(pty.slave, nix::libc::STDIN_FILENO)?;
            nix::unistd::dup2(pty.slave, nix::libc::STDOUT_FILENO)?;
            nix::unistd::dup2(pty.slave, nix::libc::STDERR_FILENO)?;
            nix::unistd::close(pty.master)?;
            nix::unistd::close(pty.slave)?;

            Command::new("bash").spawn()?.wait()?;
        }
    }

    Ok(())
}
