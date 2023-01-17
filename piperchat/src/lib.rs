pub const APP_ID: &str = "eu.mguzik.piperchat";

pub mod gui;

pub mod message;

use adw::prelude::MessageDialogExtManual;
use adw::traits::MessageDialogExt;
use adw::ResponseAppearance;
pub use message::*;

pub mod app;
pub use app::*;

use async_std::channel::{Receiver, Sender};
use async_std::io::BufReader;
use async_std::task;
use futures::channel::mpsc;
use futures::{select, AsyncBufReadExt, Sink, SinkExt, Stream, StreamExt};
use gtk::prelude::*;
use log::{debug, error, info, warn};
use tokio_tungstenite::tungstenite::Error;

use crate as pc;
use crate::app::{App, CallSide};
use crate::gui::window::Window;

type WsMessage = tokio_tungstenite::tungstenite::Message;
type PcMessage = crate::Message;

#[derive(Debug)]
enum AppState {
    Connected,
    CallReceived,
    CallRequested,
    InCall(Call),
}

#[derive(Debug)]
pub enum NetworkEvent {
    UserlistReceived(Vec<(u32, String)>),
    CallReceived(String),
}

#[derive(Debug)]
pub enum GuiEvent {
    CallStart(u32),
    CallAccepted(VideoPreference),
    CallRejected,
}

#[derive(Debug)]
pub enum VideoPreference {
    Enabled,
    Disabled,
}

#[derive(Debug)]
struct Call {
    handle: task::JoinHandle<anyhow::Result<()>>,
    app_tx: mpsc::UnboundedSender<pc::WebrtcMsg>,
}

impl Call {
    fn new(
        gst_tx: mpsc::UnboundedSender<pc::WebrtcMsg>,
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

pub async fn run(
    ws: impl Sink<WsMessage, Error = Error> + Stream<Item = Result<WsMessage, Error>>,
    mut exit_rx: mpsc::UnboundedReceiver<()>,
    network_tx: async_std::channel::Sender<NetworkEvent>,
    mut gui_rx: async_std::channel::Receiver<GuiEvent>,
) -> Result<(), anyhow::Error> {
    // Split the websocket into the Sink and Stream
    let (mut ws_sink, ws_stream) = ws.split();

    // Fuse the Stream, required for the select macro
    let mut ws_stream = ws_stream.fuse();

    let (gst_tx, mut gst_rx) = mpsc::unbounded::<pc::WebrtcMsg>();

    let (gst_exit_tx, mut gst_exit_rx) = mpsc::unbounded();

    let stdin_buf = BufReader::new(async_std::io::stdin());
    let mut lines = stdin_buf.lines().fuse();

    let mut state = AppState::Connected;

    // And now let's start our message loop
    loop {
        let ws_msg: Option<pc::Message> = select! {
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
                            for (id, user) in &userlist.users {
                                println!("- {user}: {id}");
                            }
                            println!("");
                            network_tx.send_blocking(NetworkEvent::UserlistReceived(userlist.users))?;
                        } else {
                            match state {
                                AppState::Connected => {
                                    if let PcMessage::CallReceived(pc::CallReceivedMessage { name }) = message {
                                        println!("Receiving a call from {name}");
                                        println!("Accept [Y/n]?");
                                        network_tx.send_blocking(NetworkEvent::CallReceived(name))?;
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
            ws_msg = gst_rx.select_next_some() => Some(pc::Message::Webrtc(ws_msg)),

            // user hit ctrl+c, exitting
            _ = exit_rx.select_next_some() => break,

            // input from stdin
            stdin_line = lines.select_next_some() => {
                let input = stdin_line?;
                let input = input.trim();
                match state {
                    // extract id to call to
                    AppState::Connected => {
                        let id: u32 = match input.parse() {
                            Ok(id) => id,
                            Err(_) => {
                                error!("invalid id");
                                continue
                            }
                        };
                        println!("connecting to {}", id);
                        state = AppState::CallRequested;

                        // Join the given session
                        Some(PcMessage::Call(pc::CallMessage { peer: id }))
                    },
                    // hangup the call
                    AppState::InCall(_) | AppState::CallRequested => {
                        if input == "q" {
                            state = AppState::Connected;
                            Some(PcMessage::CallHangup)
                        } else {
                            println!("To hangup, press q");
                            None
                        }
                    },
                    // answer/reject the call
                    AppState::CallReceived => {
                        if input == "y" || input == "" {
                            state = AppState::InCall(Call::new(gst_tx.clone(), CallSide::Callee, gst_exit_tx.clone())?);
                            Some(PcMessage::CallResponse(pc::CallResponseMessage::Accept))
                        } else {
                            state = AppState::Connected;
                            Some(PcMessage::CallResponse(pc::CallResponseMessage::Reject))
                        }
                    },
                }
            },

            gui_msg = gui_rx.select_next_some() => {
                match gui_msg {
                    GuiEvent::CallStart(id) => {
                        println!("connecting to {}", id);
                        state = AppState::CallRequested;

                        // Join the given session
                        Some(PcMessage::Call(pc::CallMessage { peer: id }))
                    },
                    GuiEvent::CallAccepted(video_preference) => {
                        state = AppState::InCall(Call::new(gst_tx.clone(), CallSide::Callee, gst_exit_tx.clone())?);
                        Some(PcMessage::CallResponse(pc::CallResponseMessage::Accept))
                    },
                    GuiEvent::CallRejected => {
                        state = AppState::Connected;
                        Some(PcMessage::CallResponse(pc::CallResponseMessage::Reject))
                    }
                }
            }

            _ = gst_exit_rx.select_next_some() => {
                if let AppState::InCall(call) = state {
                    let output = call.handle.await;
                    info!("Call terminated. Reason: {output:?}");
                    state = AppState::Connected;
                    Some(PcMessage::CallHangup)
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
            let message = WsMessage::Text(serde_json::to_string(&ws_msg)?);
            ws_sink.send(message).await?;
        }
    }

    ws_sink.close().await?;
    Ok(())
}

pub fn build_ui(
    app: &adw::Application,
    network_rx: Receiver<NetworkEvent>,
    gui_tx: Sender<GuiEvent>,
) {
    // Create a new custom window and show it
    let window = Window::new(&app, gui_tx);
    window.set_title(Some("Piperchat"));
    window.present();

    let event_handler = async move {
        while let Ok(event) = network_rx.recv().await {
            info!("received event: {event:?}");
            match event {
                NetworkEvent::UserlistReceived(userlist) => {
                    window.set_contacts(userlist);
                }
                NetworkEvent::CallReceived(username) => {
                    // display some dialog where user can accept/reject the message
                    let dialog = adw::MessageDialog::new(
                        Some(&window),
                        Some(&format!("Incoming call from {username}")),
                        Some(&format!("Receiving a call from {username}. Do you want to accept or reject this call?")),
                    );
                    dialog.add_responses(&[
                        ("accept", "Accept"),
                        ("accept_novideo", "Accept without video"),
                        ("reject", "Reject"),
                    ]);
                    dialog.set_response_appearance("accept", ResponseAppearance::Suggested);
                    dialog.set_response_appearance("reject", ResponseAppearance::Destructive);
                    let response = dialog.run_future().await;
                    println!("{response}");
                    match response.as_str() {
                        "accept" => {
                            window.accept_call();
                        }
                        "accept_novideo" => {
                            window.accept_call_without_video();
                        }
                        "reject" => {
                            window.reject_call();
                        }
                        _ => unreachable!("dialog response not possible"),
                    }
                }
            }
        }
    };

    gtk::glib::MainContext::default().spawn_local(event_handler);

    info!("UI built");
}
