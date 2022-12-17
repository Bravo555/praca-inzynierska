use std::env;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

#[tokio::main]
async fn main() {
    // connect and send client name
    let name = format!("{}\n", env::args().nth(1).unwrap());
    let mut socket = TcpStream::connect("127.0.0.1:2137").await.unwrap();
    socket.write_all(name.as_bytes()).await.unwrap();

    loop {
        let mut buf = vec![0u8; 1024];
        match socket.read(&mut buf).await {
            Ok(n) => {
                let message = String::from_utf8(buf).unwrap();
                println!("MESSAGE FROM SERVER:");
                println!("{message}");
            }
            Err(_) => break,
        }
    }
}
