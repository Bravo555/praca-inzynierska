use anyhow::{anyhow, bail, Context};
use async_std::io::BufReader;
use async_std::task;
use clap::{arg, Parser};
use futures::channel::mpsc;
use futures::{select, AsyncBufReadExt, Sink, SinkExt, Stream, StreamExt};
use log::{debug, info, warn};
use rand::Rng;
use std::io::{self, Write};
use tokio_tungstenite::tungstenite::Error;

use piperchat as pc;
use piperchat::app::App;

type WsMessage = tokio_tungstenite::tungstenite::Message;
type PcMessage = piperchat::Message;

#[derive(Debug, clap::Parser)]
struct Args {
    #[arg(short, long, default_value = "ws://localhost:2137")]
    server: String,
    #[arg(short, long)]
    name: String,
}

#[derive(Debug, Clone, Copy)]
pub enum AppState {
    Connected,
    CallReceived,
    CallRequested,
    InCall,
}

async fn run(
    ws: impl Sink<WsMessage, Error = Error> + Stream<Item = Result<WsMessage, Error>>,
    mut exit_rx: mpsc::UnboundedReceiver<()>,
) -> Result<(), anyhow::Error> {
    // Split the websocket into the Sink and Stream
    let (mut ws_sink, ws_stream) = ws.split();
    // Fuse the Stream, required for the select macro
    let mut ws_stream = ws_stream.fuse();

    // Create our application state
    let (app, send_gst_msg_rx, send_ws_msg_rx) = App::new()?;

    let mut send_gst_msg_rx = send_gst_msg_rx.fuse();
    let mut send_ws_msg_rx = send_ws_msg_rx.fuse();

    let stdin_buf = BufReader::new(async_std::io::stdin());
    let mut lines = stdin_buf.lines().fuse();

    let mut state = AppState::Connected;

    print!("connect_to: ");
    io::stdout().flush().unwrap();

    // And now let's start our message loop
    loop {
        let ws_msg = select! {
            // Handle the WebSocket messages here
            ws_msg = ws_stream.select_next_some() => {
                info!("received: {ws_msg:?}");
                match ws_msg? {
                    WsMessage::Close(_) => {
                        println!("peer disconnected");
                        break
                    },
                    WsMessage::Text(text) => {
                        let message: PcMessage = serde_json::from_str(&text)?;

                        if let PcMessage::UserList(userlist) = message {
                            println!("users:");
                            for (id, user) in userlist.users {
                                println!("- {user}: {id}");
                            }
                            println!("");
                        } else {
                            match state {
                                AppState::Connected => {
                                    if let PcMessage::CallReceived(pc::CallReceivedMessage { name }) = message {
                                        println!("Receiving a call from {name}");
                                        println!("Accept [Y/n]?");
                                        state = AppState::CallReceived;
                                    } else {
                                        warn!("Received another call while call pending");
                                    }
                                },
                                // peer can accept or reject
                                AppState::CallRequested => {
                                    match message {
                                        PcMessage::CallResponse(pc::CallResponseMessage::Accept) => {
                                            app.set_on_negotiation_needed();
                                            app.set_pipeline_state(gst::State::Playing);
                                            state = AppState::InCall;
                                        }
                                        PcMessage::CallResponse(pc::CallResponseMessage::Reject) => {
                                            state = AppState::Connected;
                                        },
                                        _ => {
                                            warn!("received wrong message: {message:?}");
                                        }
                                    }
                                },
                                // peer hung up
                                AppState::CallReceived => {
                                    if let PcMessage::CallHangup = message {
                                        state = AppState::Connected;
                                    }
                                },
                                // peer hungup
                                AppState::InCall => {
                                    match message {
                                        PcMessage::CallHangup => {
                                            app.set_pipeline_state(gst::State::Ready);
                                            state = AppState::Connected;
                                        },
                                        PcMessage::Webrtc(webrtc) => {
                                            app.handle_webrtc_message(webrtc).context("bad websocket message")?;
                                        },
                                        _ => {
                                            warn!("received wrong message: {message:?}");
                                        }
                                    }
                                }
                            }
                        }

                        None
                    },
                    WsMessage::Frame(_) => unreachable!(),
                    _ => None
                }
            },
            // Pass the GStreamer messages to the application control logic
            gst_msg = send_gst_msg_rx.select_next_some() => {
                debug!("pipeline message: {gst_msg:?}");
                app.handle_pipeline_message(&gst_msg)?;
                None
            },
            // Handle WebSocket messages we created asynchronously
            // to send them out now
            ws_msg = send_ws_msg_rx.select_next_some() => Some(ws_msg),

            // user hit ctrl+c, exitting
            _ = exit_rx.select_next_some() => break,

            // input from stdin
            stdin_line = lines.select_next_some() => {
                let input = stdin_line?;
                let input = input.trim();
                match state {
                    // extract id to call to
                    AppState::Connected => {
                        state = AppState::CallRequested;
                        let id: usize = input.parse()?;
                        println!("connecting to {}", id);

                        // Join the given session
                        let call_message = serde_json::to_string(&PcMessage::Call(pc::CallMessage { peer: id }))?;
                        Some(WsMessage::Text(call_message))
                    },
                    // hangup the call
                    AppState::InCall | AppState::CallRequested => {
                        state = AppState::Connected;
                        if input == "q" {
                            let disconnect_message = serde_json::to_string(&PcMessage::CallHangup)?;
                            Some(WsMessage::Text(disconnect_message))
                        } else {
                            println!("To hangup, press q");
                            None
                        }
                    },
                    // answer/reject the call
                    AppState::CallReceived => {
                        if input == "y" || input == "" {
                            state = AppState::InCall;
                            let accept_message = serde_json::to_string(&PcMessage::CallResponse(pc::CallResponseMessage::Accept))?;
                            Some(WsMessage::Text(accept_message))
                        } else {
                            state = AppState::Connected;
                            let reject_message = serde_json::to_string(&PcMessage::CallResponse(pc::CallResponseMessage::Reject))?;
                            Some(WsMessage::Text(reject_message))
                        }
                    },
                }
            }

            // Once we're done, break the loop and return
            complete => break,
        };

        // If there's a message to send out, do so now
        if let Some(ws_msg) = ws_msg {
            ws_sink.send(ws_msg).await?;
        }
    }

    ws_sink.close().await?;
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let (exit_tx, exit_rx) = mpsc::unbounded();
    ctrlc::set_handler(move || {
        exit_tx.unbounded_send(()).unwrap();
    })
    .context("Error setting Ctrl-C handler")?;
    pretty_env_logger::init();
    task::block_on(main_async(exit_rx))
}

async fn main_async(exit_rx: mpsc::UnboundedReceiver<()>) -> anyhow::Result<()> {
    // Initialize GStreamer first
    gst::init()?;

    let args = Args::parse();

    // Connect to the given server
    let (mut ws, _) = async_tungstenite::async_std::connect_async(&args.server).await?;

    println!("connected");

    // Say HELLO to the server and see if it replies with HELLO
    let id = rand::thread_rng().gen_range(10..10_000);
    println!("Registering id {} with server", id);
    let connect_message = serde_json::to_string(&PcMessage::Connect(pc::ConnectMessage {
        name: args.name.clone(),
        id,
    }))?;
    ws.send(WsMessage::Text(connect_message)).await?;

    let msg = ws
        .next()
        .await
        .ok_or_else(|| anyhow!("didn't receive anything"))??;
    let response = if let WsMessage::Text(msg) = msg {
        msg
    } else {
        bail!("bad message");
    };
    let response: PcMessage = serde_json::from_str(&response)?;
    info!("{:?}", &response);
    match response {
        PcMessage::ConnectResponse(pc::ConnectResponse::Accept) => (),
        PcMessage::ConnectResponse(pc::ConnectResponse::Reject(reason)) => {
            bail!("server rejected the connection. Reason: {reason}");
        }
        msg => bail!("Expected connection accept, received: {msg:?}"),
    }

    // All good, let's run our message loop
    run(ws, exit_rx).await
}
