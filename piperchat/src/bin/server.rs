use color_eyre::eyre::{bail, eyre};
use futures::{SinkExt, StreamExt};
use log::{error, info};
use piperchat as pc;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::{
    net::{TcpListener, TcpStream},
    select,
    sync::mpsc,
};
use tokio_tungstenite::tungstenite;
type WsMessage = tungstenite::Message;
type PcMessage = piperchat::Message;

#[derive(Debug)]
enum Command {
    SendMessage(PcMessage),
    CallReceived {
        channel: mpsc::UnboundedSender<Command>,
        name: String,
    },
    CallAccepted(mpsc::UnboundedSender<Command>),
    PeerHungup,
    CallRejected,
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
async fn main() -> color_eyre::Result<()> {
    pretty_env_logger::init();
    color_eyre::install()?;
    let listener = TcpListener::bind("0.0.0.0:2137").await.unwrap();
    let state = Arc::new(Mutex::new(State::new()));

    loop {
        let (socket, _) = listener.accept().await.unwrap();
        let state = state.clone();
        tokio::spawn(async move { process(socket, state).await.unwrap() });
    }
}

async fn process(socket: TcpStream, state: Arc<Mutex<State>>) -> color_eyre::Result<()> {
    let ws = tokio_tungstenite::accept_async(socket).await?;
    let (mut ws_sink, mut ws_stream) = ws.split();

    let message = read_message(&mut ws_stream).await?;
    let connect_message = if let PcMessage::Connect(connect_message) = message {
        connect_message
    } else {
        bail!("Expected ConnectMessage, got: {message:?}");
    };

    let name = connect_message.name;
    let id = connect_message.id;
    // check if there's no client with the same name
    if state
        .lock()
        .unwrap()
        .users
        .values()
        .find(|user| user.name == name)
        .is_some()
    {
        send_message(
            &mut ws_sink,
            &PcMessage::ConnectResponse(pc::ConnectResponse::Reject(
                "User with this name already exist. Please pick a different name".to_string(),
            )),
        )
        .await?;
        bail!("User didn't connect");
    }

    send_message(
        &mut ws_sink,
        &PcMessage::ConnectResponse(pc::ConnectResponse::Accept),
    )
    .await?;
    info!("{id} connected.");

    // construct user
    let (tx, mut rx) = mpsc::unbounded_channel();
    let user = User {
        name: name.clone(),
        id,
        tx: tx.clone(),
    };

    {
        let mut state = state.lock().unwrap();
        let users = &mut state.users;
        users.insert(user.id, user);
        broadcast_user_list(&users);
    }

    let mut client_state = ClientState::Connected;

    loop {
        select! {
            command = rx.recv() => {
                match command {
                    Some(Command::SendMessage(message)) => {
                        send_message(&mut ws_sink, &message).await?
                    }
                    Some(Command::CallReceived{ channel: peer_sink, name }) => {
                        let message = PcMessage::CallReceived(pc::CallReceivedMessage { name: name.clone() });
                        send_message(&mut ws_sink, &message).await?;
                        client_state = ClientState::CallReceived(peer_sink);
                    },
                    Some(Command::CallAccepted(peer_sink)) => {
                        let message = PcMessage::CallResponse(pc::CallResponseMessage::Accept);
                        send_message(&mut ws_sink, &message).await?;
                        client_state = ClientState::InCall(peer_sink);
                    },
                    Some(Command::PeerHungup) => {
                        let message = PcMessage::CallHangup;
                        send_message(&mut ws_sink, &message).await?;
                        client_state = ClientState::Connected;
                    }
                    Some(Command::CallRejected) => {
                        let message = PcMessage::CallResponse(pc::CallResponseMessage::Reject);
                        send_message(&mut ws_sink, &message).await?;
                        client_state = ClientState::Connected;
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
                        let message: PcMessage = serde_json::from_str(&message)?;

                        client_state = match client_state {
                            ClientState::Connected => {
                                if let PcMessage::Call(call_message) = message {
                                    {
                                        let session_id = call_message.peer;
                                        if session_id == id {
                                            error!("Can't call self!");
                                            break;
                                        }
                                        info!("{id} requested call with {session_id}");

                                        let state = state.lock().unwrap();
                                        let peer_tx = &state.users.get(&session_id).ok_or(eyre!("no such session"))?.tx;
                                        peer_tx.send(Command::CallReceived{channel: tx.clone(), name: name.clone()})?;
                                        let session_peer = peer_tx.clone();
                                        ClientState::CallRequested(session_peer)
                                    }
                                } else {
                                    error!("Wrong message from user {id}: {message:?}");
                                    ClientState::Connected
                                }
                            },
                            ClientState::CallReceived(peer_sink) => {
                                // we either accept or reject the call
                                if let PcMessage::CallResponse(call_response) = message {
                                    match call_response {
                                        pc::CallResponseMessage::Accept => {
                                            // let the peer know we accepted and transition to incall state
                                            peer_sink.send(Command::CallAccepted(tx.clone()))?;
                                            ClientState::InCall(peer_sink)
                                        },
                                        pc::CallResponseMessage::Reject => {
                                            peer_sink.send(Command::CallRejected)?;
                                            ClientState::Connected
                                        },
                                    }
                                } else {
                                    error!("Wrong message from client {id}: {message:?}");
                                    ClientState::CallReceived(peer_sink)
                                }
                            }
                            ClientState::CallRequested(peer) => {
                                // At this point we need to listen for peer to accept or reject the call
                                // The only thing we can do is hang up
                                match message {
                                    PcMessage::CallHangup => ClientState::Connected,
                                    _ => {
                                        error!("Wrong message from user {id}: {message:?}");
                                        ClientState::CallRequested(peer)
                                    }
                                }
                                // ws_sink.send(WsMessage::text("SESSION_OK")).await?;
                            },
                            ClientState::InCall(peer) => {
                                // We either send a message to peer, or hangup
                                match message {
                                    pc::Message::Webrtc(_) => {
                                        peer.send(Command::SendMessage(message.clone()))?;
                                        ClientState::InCall(peer)
                                    }
                                    pc::Message::CallHangup => {
                                        peer.send(Command::PeerHungup)?;
                                        ClientState::Connected
                                    },
                                    _ => {
                                        error!("wrong message from user {id}: {message:?}");
                                        ClientState::InCall(peer)
                                    }
                                }
                            }
                        };
                    },
                    Some(Err(err)) => {
                        error!("client={}, error={}", id, err);
                        match client_state {
                            ClientState::InCall(peer) | ClientState::CallRequested(peer) => {
                                peer.send(Command::PeerHungup)?;
                            }
                            ClientState::CallReceived(peer) => {
                                peer.send(Command::CallRejected)?;
                            },
                            ClientState::Connected => ()
                        }
                        break;
                    }
                    None => break,
                    _ => ()
                }
            }
        }
    }

    info!("{id} disconnected.");

    {
        let mut state = state.lock().unwrap();
        let users = &mut state.users;
        users.remove(&id);
        broadcast_user_list(&users);
    }

    Ok(())
}

fn broadcast_user_list(users: &HashMap<usize, User>) {
    let userlist_message = PcMessage::UserList(pc::UserList {
        users: users
            .values()
            .map(|user| (user.id, user.name.clone()))
            .collect(),
    });

    // send message to all users
    for user in users.values() {
        user.tx
            // TODO: fix unnecessary copies
            .send(Command::SendMessage(userlist_message.clone()))
            .unwrap();
    }
}

async fn read_message<S>(ws_stream: &mut S) -> color_eyre::Result<PcMessage>
where
    S: StreamExt<Item = tungstenite::Result<WsMessage>> + Unpin,
{
    let message = if let Some(Ok(WsMessage::Text(payload))) = ws_stream.next().await {
        payload
    } else {
        bail!("Received WebSocket Frame is not a text frame");
    };
    let message: PcMessage = serde_json::from_str(&message)?;
    info!("received: {message:?}");

    Ok(message)
}

async fn send_message<S>(ws_sink: &mut S, message: &PcMessage) -> color_eyre::Result<()>
where
    S: SinkExt<WsMessage, Error = tungstenite::Error> + Unpin,
{
    info!("sending: {message:?}");
    let message = serde_json::to_string(&message)?;
    ws_sink.send(WsMessage::text(&message)).await?;

    Ok(())
}

#[derive(Debug)]
enum ClientState {
    Connected,
    CallRequested(mpsc::UnboundedSender<Command>),
    CallReceived(mpsc::UnboundedSender<Command>),
    InCall(mpsc::UnboundedSender<Command>),
}
