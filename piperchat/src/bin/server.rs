use std::sync::{Arc, Mutex};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc::{self, Sender, UnboundedSender},
};

#[derive(Debug)]
struct User {
    name: String,
    tx: UnboundedSender<String>,
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

async fn process(mut socket: TcpStream, users: Arc<Mutex<Vec<User>>>) {
    // receive user name as bytes into socket
    let mut buffer = vec![0; 1024];
    socket.read(&mut buffer).await.unwrap();

    // get user name as string up until newline character
    let newline = buffer
        .iter()
        .position(|byte| *byte == 0x0a)
        .expect("invalid username");
    buffer.truncate(newline);

    let name = String::from_utf8(buffer).unwrap();

    // construct user
    let (tx, mut rx) = mpsc::unbounded_channel();
    let user = User {
        name: name.clone(),
        tx: tx.clone(),
    };
    {
        let mut users = users.lock().unwrap();
        users.push(user);
    }

    broadcast_user_list(users.clone()).await;

    loop {
        let message = rx.recv().await;
        match message {
            Some(message) => socket.write_all(message.as_bytes()).await.unwrap(),
            None => break,
        }
    }

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
