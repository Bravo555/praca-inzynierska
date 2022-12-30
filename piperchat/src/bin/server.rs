use anyhow::{anyhow, bail};
use futures::{SinkExt, StreamExt};
use log::info;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::{
    net::{TcpListener, TcpStream},
    select,
    sync::mpsc,
};
type WsMessage = tokio_tungstenite::tungstenite::Message;

#[derive(Debug)]
enum Command {
    SendMessage(String),
    SessionJoin(mpsc::UnboundedSender<Command>),
}

struct State {
    users: HashMap<usize, User>,
}

impl State {
    fn new() -> Self {
        State {
            users: HashMap::new(),
        }
    }
}

#[derive(Debug)]
struct User {
    name: String,
    id: usize,
    tx: mpsc::UnboundedSender<Command>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::init();
    let listener = TcpListener::bind("0.0.0.0:2137").await.unwrap();
    let state = Arc::new(Mutex::new(State::new()));

    loop {
        let (socket, _) = listener.accept().await.unwrap();
        let state = state.clone();
        tokio::spawn(async move {
            process(socket, state).await.unwrap();
        });
    }
}

async fn process(socket: TcpStream, state: Arc<Mutex<State>>) -> anyhow::Result<()> {
    let ws = tokio_tungstenite::accept_async(socket).await?;
    let (mut ws_sink, mut ws_stream) = ws.split();

    // receive user name as bytes into socket
    let message = match ws_stream.next().await {
        Some(Ok(WsMessage::Text(payload))) => payload,
        _ => {
            bail!("bad message");
        }
    };

    let (command, id) = message.split_once(" ").unwrap();
    if command != "HELLO" {
        bail!("Connect message invalid")
    }

    let id: usize = id.parse()?;
    ws_sink.send(WsMessage::text("HELLO")).await?;
    info!("got HELLO");

    println!("{id} connected.");

    // construct user
    let (tx, mut rx) = mpsc::unbounded_channel();
    let user = User {
        name: "john".to_owned(),
        id,
        tx: tx.clone(),
    };

    {
        let mut state = state.lock().unwrap();
        let users = &mut state.users;
        users.insert(user.id, user);
        // broadcast_user_list(&users);
    }

    let mut session_peer: Option<mpsc::UnboundedSender<Command>> = None;

    loop {
        select! {
            command = rx.recv() => {
                info!("sending: {message:?}");
                match command {
                    Some(Command::SendMessage(message)) => {
                        ws_sink.send(message.into()).await.unwrap();
                    }
                    Some(Command::SessionJoin(peer_sink)) => {
                        session_peer = Some(peer_sink);
                    }
                    None => break,
                }
            },

            // message from user socket
            // if not in session:
            // - SESSION_OK to create session
            // if in session:
            //   just reroute everything to session peer
            message = ws_stream.next() => {
                match message {
                    Some(Ok(WsMessage::Text(message))) => {
                        info!("received: {message:?}");

                        if let Some(peer) = session_peer.as_ref() {
                            peer.send(Command::SendMessage(message.clone()))?;
                        }

                        if session_peer.is_none() {
                            let (command, id) = message.split_once(" ").unwrap();
                            if command == "SESSION" {
                                {
                                    let state = state.lock().unwrap();
                                    let session_id = id.parse()?;
                                    let peer_tx = &state.users.get(&session_id).ok_or(anyhow!("no such session"))?.tx;
                                    peer_tx.send(Command::SessionJoin(tx.clone()))?;
                                    session_peer = Some(peer_tx.clone());

                                }
                                ws_sink.send(WsMessage::text("SESSION_OK")).await?;
                            }
                        }
                    },
                    _ => break
                }
            }
        }
    }

    println!("{id} disconnected.");

    {
        let mut state = state.lock().unwrap();
        let users = &mut state.users;
        users.remove(&id);
        // broadcast_user_list(&users);
    }

    Ok(())
}

fn broadcast_user_list(users: &HashMap<usize, User>) {
    // construct message with all connected users
    let message = {
        let user_names: Vec<_> = users
            .values()
            .map(|user| format!("- {}: {}", user.name, user.id))
            .collect();
        let user_names = user_names.join("\n");
        format!("sessions:\n{user_names}")
    };

    // send message to all users
    for user in users.values() {
        user.tx.send(Command::SendMessage(message.clone())).unwrap();
    }
}
