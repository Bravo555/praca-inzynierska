use anyhow::{anyhow, bail, Context};
use clap::Parser;
use futures::channel::mpsc;
use futures::{SinkExt, StreamExt};
use gtk::prelude::ApplicationExtManual;
use gtk::{gio, prelude::ApplicationExt};
use log::{debug, info};
use piperchat as pc;
use piperchat::APP_ID;
use piperchat::{GuiEvent, NetworkEvent};
use rand::Rng;

type WsMessage = async_tungstenite::tungstenite::Message;

#[derive(Debug, clap::Parser)]
struct Args {
    #[arg(short, long, default_value = "ws://localhost:2137")]
    server: String,
    #[arg(short, long)]
    name: String,
}

fn main() -> anyhow::Result<()> {
    let (exit_tx, exit_rx) = mpsc::unbounded();
    ctrlc::set_handler(move || {
        exit_tx.unbounded_send(()).unwrap();
    })
    .context("Error setting Ctrl-C handler")?;
    pretty_env_logger::init();

    let (network_tx, network_rx) = async_std::channel::unbounded::<NetworkEvent>();
    let (gui_tx, gui_rx) = async_std::channel::unbounded::<GuiEvent>();

    // So apparently GTK, when executing multiple instances of an application with the same APP_ID, will take the window
    // from the newly spawned instance and give it to the previously spawned instance, and then exit the new instance,
    // so there's only one process controlling all the windows. This might be helpful with saving system resources for
    // some applications, if they are written in a sane way, but as this project is very cursed and I have no idea how
    // to write GTK applications, I'm going to have none of that and pretend to GTK that these are separate applications
    // so that it doesn't hijack the window and prematurely kill the process.
    let rand_proc_id: String = rand::thread_rng()
        .sample_iter(rand::distributions::Uniform::new(
            char::from(97),
            char::from(122),
        ))
        .take(8)
        .map(char::from)
        .collect();
    let app_id = format!("{APP_ID}.{rand_proc_id}");
    debug!("appid = {app_id}");

    // GTK
    // Register and include resources
    gio::resources_register_include!("piperchat.gresource").expect("Failed to register resources.");

    // Create a new application
    let app = adw::Application::builder().application_id(&app_id).build();

    // Connect signals
    app.connect_activate(move |app| pc::build_ui(app, network_rx.clone(), gui_tx.clone()));

    // network client
    gtk::glib::MainContext::default().spawn_local(async move {
        main_async(exit_rx, network_tx, gui_rx).await.unwrap();
    });

    // Run the application
    app.run_with_args::<&str>(&[]);

    Ok(())
}

async fn main_async(
    exit_rx: mpsc::UnboundedReceiver<()>,
    network_tx: async_std::channel::Sender<NetworkEvent>,
    gui_rx: async_std::channel::Receiver<GuiEvent>,
) -> anyhow::Result<()> {
    // Initialize GStreamer first
    gst::init()?;

    let args = Args::parse();

    // Connect to the given server
    let (mut ws, _) = async_tungstenite::async_std::connect_async(&args.server).await?;

    println!("connected");

    // Say HELLO to the server and see if it replies with HELLO
    let id = rand::thread_rng().gen_range(10..10_000);
    println!("Registering id {} with server", id);
    let connect_message = serde_json::to_string(&pc::Message::Connect(pc::ConnectMessage {
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
    let response: pc::Message = serde_json::from_str(&response)?;
    info!("{:?}", &response);
    match response {
        pc::Message::ConnectResponse(pc::ConnectResponse::Accept) => (),
        pc::Message::ConnectResponse(pc::ConnectResponse::Reject(reason)) => {
            bail!("server rejected the connection. Reason: {reason}");
        }
        msg => bail!("Expected connection accept, received: {msg:?}"),
    }

    // All good, let's run our message loop
    pc::run(ws, exit_rx, network_tx, gui_rx).await
}
