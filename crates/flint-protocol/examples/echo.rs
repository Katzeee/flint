//! A bounded TCP peer for the cross-language contract tests, not a backend.
use flint_protocol::EnvelopeCodec;
use futures_util::{SinkExt, StreamExt};
use std::io::Write;
use tokio_util::codec::Framed;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let count: usize = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "3".into())
        .parse()?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    println!("{}", listener.local_addr()?.port());
    std::io::stdout().flush()?;
    for _ in 0..count {
        let (socket, _) = listener.accept().await?;
        let mut connection = Framed::new(socket, EnvelopeCodec::default());
        while let Some(message) = connection.next().await {
            connection.send(message?).await?;
        }
    }
    Ok(())
}
