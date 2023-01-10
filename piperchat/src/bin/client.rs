use anyhow::{anyhow, bail, Context};
use async_std::io::BufReader;
use async_std::task;
use clap::{arg, Parser};
use futures::channel::mpsc;
use futures::{select, AsyncBufReadExt, Sink, SinkExt, Stream, StreamExt};
use log::{debug, error, info, warn};
use rand::Rng;
use tokio_tungstenite::tungstenite::Error;

use piperchat as pc;
use piperchat::app::{App, CallSide};

type WsMessage = tokio_tungstenite::tungstenite::Message;
type PcMessage = piperchat::Message;

#[derive(Debug, clap::Parser)]
struct Args {
    #[arg(short, long, default_value = "ws://localhost:2137")]
    server: String,
    #[arg(short, long)]
    name: String,
}

#[derive(Debug)]
enum AppState {
    Connected,
    CallReceived,
    CallRequested,
    InCall(Call),
}

#[derive(Debug)]
struct Call {
    handle: task::JoinHandle<anyhow::Result<()>>,
    app_tx: mpsc::UnboundedSender<pc::WebrtcMsg>,
}

impl Call {
    fn new(
        gst_tx: mpsc::UnboundedSender<WsMessage>,
        callside: CallSide,
        exit_tx: mpsc::UnboundedSender<()>,
    ) -> anyhow::Result<Self> {
        let (app_tx, app_rx) = mpsc::unbounded();
        let handle = task::spawn(async move {
            let (gstreamer, gst_bus, gst_rx) = App::new(callside)?;
            let mut gst_bus = gst_bus.fuse();
            let mut gst_rx = gst_rx.fuse();
            let mut app_rx = app_rx.fuse();

            loop {
                select! {
                    gst_msg = gst_bus.select_next_some() => {
                        debug!("pipeline message: {gst_msg:?}");
                        if let Err(err) = gstreamer.handle_pipeline_message(&gst_msg) {
                            error!("{err}");
                            break;
                        }
                    }
                    // send websocket messages emitted by gst and exit if gstreamer exited
                    ws_msg = gst_rx.next() => {
                        match ws_msg {
                            Some(ws_msg) => gst_tx.unbounded_send(ws_msg)?,
                            None => break
                        }
                    }
                    ws_msg = app_rx.next() => {
                        match ws_msg {
                            Some(ws_msg) => gstreamer.handle_webrtc_message(ws_msg)?,
                            None => break,
                        }
                    }
                }
            }

            info!("running drop on gst task");
            exit_tx.unbounded_send(())?;

            Ok(())
        });

        Ok(Call { handle, app_tx })
    }
}

async fn run(
    ws: impl Sink<WsMessage, Error = Error> + Stream<Item = Result<WsMessage, Error>>,
    mut exit_rx: mpsc::UnboundedReceiver<()>,
) -> Result<(), anyhow::Error> {
    // Split the websocket into the Sink and Stream
    let (mut ws_sink, ws_stream) = ws.split();
    // Fuse the Stream, required for the select macro
    let mut ws_stream = ws_stream.fuse();

    let (gst_tx, mut gst_rx) = mpsc::unbounded::<WsMessage>();

    let (gst_exit_tx, mut gst_exit_rx) = mpsc::unbounded();

    let stdin_buf = BufReader::new(async_std::io::stdin());
    let mut lines = stdin_buf.lines().fuse();

    let mut state = AppState::Connected;

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
                                            state = AppState::InCall(Call::new(gst_tx.clone(), CallSide::Caller, gst_exit_tx.clone())?);
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
                                AppState::InCall(ref call) => {
                                    match message {
                                        PcMessage::CallHangup => {
                                            state = AppState::Connected;
                                        },
                                        PcMessage::Webrtc(webrtc) => {
                                            call.app_tx.unbounded_send(webrtc).unwrap();
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
            // Handle WebSocket messages we created asynchronously to send them out now
            ws_msg = gst_rx.select_next_some() => Some(ws_msg),

            // user hit ctrl+c, exitting
            _ = exit_rx.select_next_some() => break,

            // input from stdin
            stdin_line = lines.select_next_some() => {
                let input = stdin_line?;
                let input = input.trim();
                match state {
                    // extract id to call to
                    AppState::Connected => {
                        let id: usize = match input.parse() {
                            Ok(id) => id,
                            Err(_) => {
                                error!("invalid id");
                                continue
                            }
                        };
                        println!("connecting to {}", id);
                        state = AppState::CallRequested;

                        // Join the given session
                        let call_message = serde_json::to_string(&PcMessage::Call(pc::CallMessage { peer: id }))?;
                        Some(WsMessage::Text(call_message))
                    },
                    // hangup the call
                    AppState::InCall(_) | AppState::CallRequested => {
                        if input == "q" {
                            let disconnect_message = serde_json::to_string(&PcMessage::CallHangup)?;
                            state = AppState::Connected;
                            Some(WsMessage::Text(disconnect_message))
                        } else {
                            println!("To hangup, press q");
                            None
                        }
                    },
                    // answer/reject the call
                    AppState::CallReceived => {
                        if input == "y" || input == "" {
                            state = AppState::InCall(Call::new(gst_tx.clone(), CallSide::Callee, gst_exit_tx.clone())?);
                            let accept_message = serde_json::to_string(&PcMessage::CallResponse(pc::CallResponseMessage::Accept))?;
                            Some(WsMessage::Text(accept_message))
                        } else {
                            state = AppState::Connected;
                            let reject_message = serde_json::to_string(&PcMessage::CallResponse(pc::CallResponseMessage::Reject))?;
                            Some(WsMessage::Text(reject_message))
                        }
                    },
                }
            },

            _ = gst_exit_rx.select_next_some() => {
                if let AppState::InCall(call) = state {
                    let output = call.handle.await;
                    info!("Call terminated. Reason: {output:?}");
                    state = AppState::Connected;
                    let disconnect_message = serde_json::to_string(&PcMessage::CallHangup)?;
                    Some(WsMessage::Text(disconnect_message))
                } else {
                    error!("Gstreamer exit received while not in call");
                    None
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
