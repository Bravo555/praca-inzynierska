use std::sync::{Arc, Mutex};

use futures_util::{SinkExt, StreamExt};
use tokio::{
    net::{TcpListener, TcpStream},
    select,
    sync::mpsc,
};
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug)]
struct User {
    name: String,
    tx: mpsc::UnboundedSender<String>,
}

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("0.0.0.0:2137").await.unwrap();
    let connected_users = Arc::new(Mutex::new(Vec::<User>::new()));

    loop {
        let (socket, _) = listener.accept().await.unwrap();
        let connected_users = connected_users.clone();
        tokio::spawn(async move {
            process(socket, connected_users).await;
        });
    }
}

async fn process(socket: TcpStream, users: Arc<Mutex<Vec<User>>>) {
    let ws = tokio_tungstenite::accept_async(socket).await.unwrap();
    let (mut ws_sink, mut ws_stream) = ws.split();

    // receive user name as bytes into socket
    let name = match ws_stream.next().await {
        Some(Ok(Message::Text(name))) => name,
        _ => {
            eprintln!("user didn't provide a name");
            return;
        }
    };
    let name = name.trim();
    println!("{name} connected.");

    // construct user
    let (tx, mut rx) = mpsc::unbounded_channel();
    let user = User {
        name: name.to_owned(),
        tx: tx.clone(),
    };

    {
        let mut users = users.lock().unwrap();
        users.push(user);
    }

    broadcast_user_list(users.clone()).await;

    loop {
        select! {
            message = rx.recv() => {
                match message {
                    Some(message) => {
                        ws_sink.send(message.into()).await.unwrap();
                    }
                    None => break,
                }
            },

            message = ws_stream.next() => {
                match message {
                    Some(Ok(message)) => (),
                    _ => break
                }
            }
        }
    }

    println!("{name} disconnected.");

    {
        let mut users = users.lock().unwrap();
        let idx = users.iter().position(|u| u.name == name).unwrap();
        users.remove(idx);
    }

    broadcast_user_list(users.clone()).await;
}

async fn broadcast_user_list(users: Arc<Mutex<Vec<User>>>) {
    // construct message with all connected users
    let message = {
        let users = users.lock().unwrap();
        let user_names: Vec<_> = users
            .iter()
            .map(|user| format!("- {}", user.name))
            .collect();
        let user_names = user_names.join("\n");
        format!("CONNECTED USERS:\n{user_names}")
    };

    // send message to all users
    let users = users.lock().unwrap();
    for user in &*users {
        user.tx.send(message.clone()).unwrap();
    }
}
