use futures_util::{SinkExt, StreamExt};
use std::env;
use tokio_tungstenite::tungstenite::Message;

#[tokio::main]
async fn main() {
    // connect and send client name
    let name = env::args().nth(1).expect("A user name is required");
    let (ws_stream, _) = tokio_tungstenite::connect_async("ws://127.0.0.1:2137")
        .await
        .unwrap();
    let (mut write, read) = ws_stream.split();
    write.send(name.into()).await.unwrap();
    read.for_each(|msg| async {
        match msg {
            Ok(Message::Text(message)) => {
                println!("MESSAGE FROM SERVER:");
                println!("{message}");
            }
            _ => (),
        }
    })
    .await;
}
