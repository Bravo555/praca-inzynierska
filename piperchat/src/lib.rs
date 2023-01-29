pub const APP_ID: &str = "eu.mguzik.piperchat";

pub mod gui;
pub mod message;
pub mod session;

use gtk::glib::MainContext;
pub use message::Message;
use message::*;

use adw::prelude::MessageDialogExtManual;
use adw::traits::MessageDialogExt;
use adw::{MessageDialog, ResponseAppearance};

use async_std::channel::{Receiver, Sender};
use async_std::io::BufReader;
use async_std::task;
use futures::channel::mpsc;
use futures::{select, AsyncBufReadExt, Sink, SinkExt, Stream, StreamExt};
use gtk::prelude::*;
use log::{debug, error, info, warn};
use tokio_tungstenite::tungstenite::Error;

use gui::window::Window;
use session::{App, CallSide};

type WsMessage = tokio_tungstenite::tungstenite::Message;
type PcMessage = message::Message;

#[derive(Debug)]
enum AppState {
    Connected,
    CallReceived(String),
    CallRequested(String),
    InCall(Call),
}

#[derive(Debug)]
pub enum NetworkEvent {
    UserlistReceived(Vec<(u32, String)>),
    CallReceived(String),
    CallAccepted,
    CallRejected(String),
    CallHangup(String),
}

#[derive(Debug)]
pub enum GuiEvent {
    CallStart(u32, String),
    CallAccepted(VideoPreference),
    CallRejected,
    NameEntered(String),
}

#[derive(Debug)]
pub enum NetworkCommand {
    CallStart(u32, String),
    CallAccept(VideoPreference),
    CallReject,
    CallHangup,
    Connect(String),
}

#[derive(Debug)]
pub enum VideoPreference {
    Enabled,
    Disabled,
}

#[derive(Debug)]
struct Call {
    handle: task::JoinHandle<anyhow::Result<()>>,
    app_tx: mpsc::UnboundedSender<message::WebrtcMsg>,
}

impl Call {
    fn new(
        gst_tx: mpsc::UnboundedSender<message::WebrtcMsg>,
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
    mut network_command_rx: async_std::channel::Receiver<NetworkCommand>,
) -> Result<(), anyhow::Error> {
    // Split the websocket into the Sink and Stream
    let (mut ws_sink, ws_stream) = ws.split();

    // Fuse the Stream, required for the select macro
    let mut ws_stream = ws_stream.fuse();

    let (gst_tx, mut gst_rx) = mpsc::unbounded::<message::WebrtcMsg>();

    let (gst_exit_tx, mut gst_exit_rx) = mpsc::unbounded();

    let stdin_buf = BufReader::new(async_std::io::stdin());
    let mut lines = stdin_buf.lines().fuse();

    let mut state = AppState::Connected;

    // And now let's start our message loop
    loop {
        let ws_msg: Option<PcMessage> = select! {
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
                            println!();
                            network_tx.send_blocking(NetworkEvent::UserlistReceived(userlist.users))?;
                        } else {
                            match state {
                                AppState::Connected => {
                                    if let PcMessage::CallReceived(message::CallReceivedMessage { name }) = message {
                                        println!("Receiving a call from {name}");
                                        println!("Accept [Y/n]?");
                                        network_tx.send_blocking(NetworkEvent::CallReceived(name.clone()))?;
                                        state = AppState::CallReceived(name);
                                    } else {
                                        warn!("Received another call while call pending");
                                    }
                                },
                                // peer can accept or reject
                                AppState::CallRequested(ref name) => {
                                    match message {
                                        PcMessage::CallResponse(CallResponseMessage::Accept) => {
                                            network_tx.send_blocking(NetworkEvent::CallAccepted)?;
                                            state = AppState::InCall(Call::new(gst_tx.clone(), CallSide::Caller, gst_exit_tx.clone())?);
                                        }
                                        PcMessage::CallResponse(CallResponseMessage::Reject) => {
                                            network_tx.send_blocking(NetworkEvent::CallRejected(name.clone()))?;
                                            state = AppState::Connected;
                                        },
                                        _ => {
                                            warn!("received wrong message: {message:?}");
                                        }
                                    }
                                },
                                // peer hung up
                                AppState::CallReceived(ref name) => {
                                    if let PcMessage::CallHangup = message {
                                        network_tx.send_blocking(NetworkEvent::CallHangup(name.clone()))?;
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
            ws_msg = gst_rx.select_next_some() => Some(Message::Webrtc(ws_msg)),

            // user hit ctrl+c, exitting
            _ = exit_rx.select_next_some() => break,

            // input from stdin
            // stdin_line = lines.select_next_some() => {
            //     let input = stdin_line?;
            //     let input = input.trim();
            //     match state {
            //         // extract id to call to
            //         AppState::Connected => {
            //             let id: u32 = match input.parse() {
            //                 Ok(id) => id,
            //                 Err(_) => {
            //                     error!("invalid id");
            //                     continue
            //                 }
            //             };
            //             println!("connecting to {}", id);
            //             state = AppState::CallRequested;

            //             // Join the given session
            //             Some(PcMessage::Call(CallMessage { peer: id }))
            //         },
            //         // hangup the call
            //         AppState::InCall(_) | AppState::CallRequested => {
            //             if input == "q" {
            //                 state = AppState::Connected;
            //                 Some(PcMessage::CallHangup)
            //             } else {
            //                 println!("To hangup, press q");
            //                 None
            //             }
            //         },
            //         // answer/reject the call
            //         AppState::CallReceived => {
            //             if input == "y" || input.is_empty() {
            //                 state = AppState::InCall(Call::new(gst_tx.clone(), CallSide::Callee, gst_exit_tx.clone())?);
            //                 Some(PcMessage::CallResponse(CallResponseMessage::Accept))
            //             } else {
            //                 state = AppState::Connected;
            //                 Some(PcMessage::CallResponse(CallResponseMessage::Reject))
            //             }
            //         },
            //     }
            // },

            command = network_command_rx.select_next_some() => {
                match command {
                    NetworkCommand::CallStart(id, name) => {
                        println!("connecting to {}", id);
                        state = AppState::CallRequested(name);

                        // Join the given session
                        Some(PcMessage::Call(CallMessage { peer: id }))
                    },
                    NetworkCommand::CallAccept(video_preference) => {
                        state = AppState::InCall(Call::new(gst_tx.clone(), CallSide::Callee, gst_exit_tx.clone())?);
                        Some(PcMessage::CallResponse(CallResponseMessage::Accept))
                    },
                    NetworkCommand::CallReject => {
                        state = AppState::Connected;
                        Some(PcMessage::CallResponse(CallResponseMessage::Reject))
                    },
                    NetworkCommand::CallHangup => {
                        state = AppState::Connected;
                        Some(PcMessage::CallHangup)
                    },
                    _ => {None}
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
            info!("sending: {ws_msg:?}");
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
    network_command_tx: Sender<NetworkCommand>,
) {
    let (gui_tx, gui_rx) = async_std::channel::unbounded::<GuiEvent>();

    // Create a new custom window and show it
    let window = Window::new(app, gui_tx);
    window.set_title(Some("Piperchat"));
    window.present();

    let handler = EventHandler {
        gui_rx,
        network_rx,
        network_command_tx,
        window,
        current_dialog: None,
    };
    handler.start();

    info!("UI built");
}

struct EventHandler {
    gui_rx: Receiver<GuiEvent>,
    network_rx: Receiver<NetworkEvent>,
    network_command_tx: Sender<NetworkCommand>,
    current_dialog: Option<MessageDialog>,
    window: Window,
}

impl EventHandler {
    fn start(mut self) {
        let event_handler = async move {
            loop {
                select! {
                    network_event = self.network_rx.next() => {
                        match network_event {
                            Some(event) => self.handle_network_event(event).await,
                            None => break
                        }
                    },
                    gui_event = self.gui_rx.next() => {
                        match gui_event {
                            Some(event) => self.handle_gui_event(event).await,
                            None => break
                        }
                    }
                }
            }
        };

        MainContext::default().spawn_local(event_handler);
    }
    async fn handle_network_event(&mut self, event: NetworkEvent) {
        info!("received network event: {event:?}");
        match event {
            NetworkEvent::UserlistReceived(userlist) => {
                self.window.set_contacts(userlist);
            }
            NetworkEvent::CallReceived(name) => {
                // display a dialog where user can accept/reject the message
                let dialog = adw::MessageDialog::new(
                    Some(&self.window),
                    Some(&format!("Incoming call from {name}")),
                    Some(&format!(
                        "Receiving a call from {name}. Do you want to accept or reject this call?"
                    )),
                );
                dialog.add_responses(&[("accept", "Accept"), ("reject", "Reject")]);
                dialog.set_response_appearance("accept", ResponseAppearance::Suggested);
                dialog.set_response_appearance("reject", ResponseAppearance::Destructive);
                let window = self.window.clone();

                dialog.run_async(None, move |_obj, response| match response {
                    "accept" => {
                        window.accept_call();
                    }
                    "reject" => {
                        window.reject_call();
                    }
                    // here dialog got closed from outside, that means sender hung up
                    _ => {
                        let dialog = adw::MessageDialog::new(
                            Some(&window),
                            Some(&format!("Caller hung up.")),
                            Some(&format!(
                                "The call from the user {name} terminated. Caller hung up."
                            )),
                        );

                        dialog.add_responses(&[("ok", "OK")]);
                        dialog.run_async(None, move |obj, response| {});
                    }
                });
                self.current_dialog = Some(dialog);
            }
            NetworkEvent::CallHangup(name) => {
                info!("Received hangup");
                if let Some(dialog) = self.current_dialog.take() {
                    dialog.close();
                }
            }
            NetworkEvent::CallAccepted => {
                if let Some(dialog) = self.current_dialog.take() {
                    info!("CLOSING");
                    dialog.close();
                }
            }
            NetworkEvent::CallRejected(name) => {
                if let Some(dialog) = self.current_dialog.take() {
                    info!("CLOSING");
                    dialog.close();
                }

                let dialog = adw::MessageDialog::new(
                    Some(&self.window),
                    Some(&format!("Call rejected")),
                    Some(&format!("Recepient {name} rejected the call.")),
                );

                dialog.add_responses(&[("ok", "OK")]);
                dialog.run_async(None, move |obj, response| {});
            }
        }
    }

    async fn handle_gui_event(&mut self, event: GuiEvent) {
        match event {
            GuiEvent::CallStart(id, name) => {
                let dialog = adw::MessageDialog::new(
                    Some(&self.window),
                    Some(&format!("Calling {name}")),
                    Some(&format!(
                    "Waiting for a response from {name}. You can wait for the answer or hangup."
                )),
                );
                dialog.add_responses(&[("hangup", "Hang up")]);
                dialog.set_response_appearance("hangup", ResponseAppearance::Destructive);

                let network_command_tx = self.network_command_tx.clone();
                dialog.run_async(None, move |obj, response| {
                    if response == "hangup" {
                        info!("SENDING HANGUP");
                        network_command_tx
                            .send_blocking(NetworkCommand::CallHangup)
                            .unwrap();
                    }
                });
                self.current_dialog = Some(dialog);

                self.network_command_tx
                    .send_blocking(NetworkCommand::CallStart(id, name))
                    .unwrap();
            }
            GuiEvent::CallAccepted(preference) => {
                self.network_command_tx
                    .send_blocking(NetworkCommand::CallAccept(preference))
                    .unwrap();
            }

            GuiEvent::CallRejected => {
                self.network_command_tx
                    .send_blocking(NetworkCommand::CallReject)
                    .unwrap();
            }
            GuiEvent::NameEntered(name) => {
                self.network_command_tx
                    .send_blocking(NetworkCommand::Connect(name))
                    .unwrap();
            }
        }
    }
}
