use anyhow::{anyhow, bail, Context};
use futures::{channel::mpsc, Stream};
use gst::glib;
use gst::glib::prelude::*;
use gst::glib::Error as GError;
use gst::prelude::*;
use gst_webrtc::gst_sdp;
use log::debug;
use log::warn;
use std::sync::{Arc, Mutex, Weak};

use crate::piperchat as pc;
use crate::WebrtcMsg;

type WsMessage = async_tungstenite::tungstenite::Message;
type PcMessage = crate::Message;

const STUN_SERVER: &str = "stun://stun.l.google.com:19302";
const TURN_SERVER: &str = "turn://foo:bar@webrtc.nirbheek.in:3478";

// upgrade weak reference or return
#[macro_export]
macro_rules! upgrade_weak {
    ($x:ident, $r:expr) => {{
        match $x.upgrade() {
            Some(o) => o,
            None => return $r,
        }
    }};
    ($x:ident) => {
        upgrade_weak!($x, ())
    };
}

pub enum CallSide {
    Caller,
    Callee,
}

// Strong reference to our application state
#[derive(Debug, Clone)]
pub struct App(Arc<AppInner>);

// Weak reference to our application state
#[derive(Debug, Clone)]
struct AppWeak(Weak<AppInner>);

// Actual application state
#[derive(Debug)]
pub struct AppInner {
    pipeline: gst::Pipeline,
    webrtcbin: gst::Element,
    send_msg_tx: Mutex<mpsc::UnboundedSender<WsMessage>>,
}

// To be able to access the App's fields directly
impl std::ops::Deref for App {
    type Target = AppInner;

    fn deref(&self) -> &AppInner {
        &self.0
    }
}

impl AppWeak {
    // Try upgrading a weak reference to a strong one
    fn upgrade(&self) -> Option<App> {
        self.0.upgrade().map(App)
    }
}

impl App {
    // Downgrade the strong reference to a weak reference
    fn downgrade(&self) -> AppWeak {
        AppWeak(Arc::downgrade(&self.0))
    }

    pub fn new(
        callside: CallSide,
    ) -> Result<
        (
            Self,
            gst::bus::BusStream,
            mpsc::UnboundedReceiver<WsMessage>,
        ),
        anyhow::Error,
    > {
        // Create the GStreamer pipeline
        let pipeline = gst::parse_launch(
            "videotestsrc pattern=ball is-live=true ! vp8enc deadline=1 ! rtpvp8pay pt=96 ! webrtcbin. \
            audiotestsrc is-live=true ! opusenc ! rtpopuspay pt=97 ! webrtcbin. \
            webrtcbin name=webrtcbin",
        )?;

        // Downcast from gst::Element to gst::Pipeline
        let pipeline = pipeline
            .downcast::<gst::Pipeline>()
            .expect("not a pipeline");

        // Get access to the webrtcbin by name
        let webrtcbin = pipeline.by_name("webrtcbin").expect("can't find webrtcbin");

        // Set some properties on webrtcbin
        webrtcbin.set_property_from_str("stun-server", STUN_SERVER);
        webrtcbin.set_property_from_str("turn-server", TURN_SERVER);
        webrtcbin.set_property_from_str("bundle-policy", "max-bundle");

        // Create a stream for handling the GStreamer message asynchronously
        let bus = pipeline.bus().unwrap();
        let send_gst_msg_rx = bus.stream();

        // Channel for outgoing WebSocket messages from other threads
        let (send_ws_msg_tx, send_ws_msg_rx) = mpsc::unbounded::<WsMessage>();

        let app = App(Arc::new(AppInner {
            pipeline,
            webrtcbin,
            send_msg_tx: Mutex::new(send_ws_msg_tx),
        }));

        if let CallSide::Caller = callside {
            let app_clone = app.downgrade();
            app.webrtcbin.connect_closure(
                "on-negotiation-needed",
                false,
                glib::closure!(move |_webrtcbin: &gst::Element| {
                    let app = upgrade_weak!(app_clone);
                    if let Err(err) = app.on_negotiation_needed() {
                        gst::element_error!(
                            app.pipeline,
                            gst::LibraryError::Failed,
                            ("Failed to negotiate: {:?}", err)
                        );
                    }
                }),
            );
        }

        // Whenever there is a new ICE candidate, send it to the peer
        let app_clone = app.downgrade();
        app.webrtcbin.connect_closure(
            "on-ice-candidate",
            false,
            glib::closure!(
                move |_webrtcbin: &gst::Element, mlineindex: u32, candidate: &str| {
                    let app = upgrade_weak!(app_clone);

                    if let Err(err) = app.on_ice_candidate(mlineindex, candidate) {
                        gst::element_error!(
                            app.pipeline,
                            gst::LibraryError::Failed,
                            ("Failed to send ICE candidate: {:?}", err)
                        );
                    }
                }
            ),
        );

        // Whenever there is a new stream incoming from the peer, handle it
        let app_clone = app.downgrade();
        app.webrtcbin.connect_pad_added(move |_webrtc, pad| {
            let app = upgrade_weak!(app_clone);

            if let Err(err) = app.on_incoming_stream(pad) {
                gst::element_error!(
                    app.pipeline,
                    gst::LibraryError::Failed,
                    ("Failed to handle incoming stream: {:?}", err)
                );
            }
        });

        // Asynchronously set the pipeline to Playing
        app.pipeline.call_async(|pipeline| {
            // If this fails, post an error on the bus so we exit
            if pipeline.set_state(gst::State::Playing).is_err() {
                gst::element_error!(
                    pipeline,
                    gst::LibraryError::Failed,
                    ("Failed to set pipeline to Playing")
                );
            }
        });

        // Asynchronously set the pipeline to Playing
        app.pipeline.call_async(|pipeline| {
            pipeline
                .set_state(gst::State::Playing)
                .expect("Couldn't set pipeline to Playing");
        });

        Ok((app, send_gst_msg_rx, send_ws_msg_rx))
    }

    // Handle WebSocket messages, both our own as well as WebSocket protocol messages
    pub fn handle_webrtc_message(&self, msg: pc::WebrtcMsg) -> Result<(), anyhow::Error> {
        match msg {
            WebrtcMsg::Sdp { type_, sdp } => self.handle_sdp(&type_, &sdp),
            WebrtcMsg::Ice {
                sdp_mline_index,
                candidate,
            } => self.handle_ice(sdp_mline_index, &candidate),
        }
    }

    // Handle GStreamer messages coming from the pipeline
    pub fn handle_pipeline_message(&self, message: &gst::Message) -> Result<(), anyhow::Error> {
        use gst::MessageView;

        match message.view() {
            MessageView::Error(err) => bail!(
                "Error from element {}: {} ({})",
                err.src()
                    .map(|s| String::from(s.path_string()))
                    .unwrap_or_else(|| String::from("None")),
                err.error(),
                err.debug().unwrap_or_else(|| String::from("None")),
            ),
            MessageView::Warning(warning) => {
                println!("Warning: \"{}\"", warning.debug().unwrap());
            }
            MessageView::Latency(_) => {
                let _ = self.pipeline.recalculate_latency();
            }
            _ => (),
        }

        Ok(())
    }

    // Whenever webrtcbin tells us that (re-)negotiation is needed, simply ask
    // for a new offer SDP from webrtcbin without any customization and then
    // asynchronously send it to the peer via the WebSocket connection
    pub fn on_negotiation_needed(&self) -> Result<(), anyhow::Error> {
        println!("starting negotiation");

        let app_clone = self.downgrade();
        let promise = gst::Promise::with_change_func(move |reply| {
            let app = upgrade_weak!(app_clone);

            if let Err(err) = app.on_offer_created(reply) {
                gst::element_error!(
                    app.pipeline,
                    gst::LibraryError::Failed,
                    ("Failed to send SDP offer: {:?}", err)
                );
            }
        });

        self.webrtcbin
            .emit_by_name::<()>("create-offer", &[&None::<gst::Structure>, &promise]);

        Ok(())
    }

    // Once webrtcbin has create the offer SDP for us, handle it by sending it to the peer via the
    // WebSocket connection
    pub fn on_offer_created(
        &self,
        reply: Result<Option<&gst::StructureRef>, gst::PromiseError>,
    ) -> Result<(), anyhow::Error> {
        let reply = match reply {
            Ok(Some(reply)) => reply,
            Ok(None) => {
                bail!("Offer creation future got no response");
            }
            Err(err) => {
                bail!("Offer creation future got error response: {:?}", err);
            }
        };

        debug!("{reply:?}");

        let offer = reply
            .value("offer")
            .unwrap()
            .get::<gst_webrtc::WebRTCSessionDescription>()
            .expect("Invalid argument");
        self.webrtcbin
            .emit_by_name::<()>("set-local-description", &[&offer, &None::<gst::Promise>]);

        println!(
            "sending SDP offer to peer: {}",
            offer.sdp().as_text().unwrap()
        );

        let message = serde_json::to_string(&PcMessage::Webrtc(WebrtcMsg::Sdp {
            type_: "offer".to_string(),
            sdp: offer.sdp().as_text().unwrap(),
        }))
        .unwrap();

        self.send_msg_tx
            .lock()
            .unwrap()
            .unbounded_send(WsMessage::Text(message))
            .context("Failed to send SDP offer")?;

        Ok(())
    }

    // Once webrtcbin has create the answer SDP for us, handle it by sending it to the peer via the
    // WebSocket connection
    fn on_answer_created(
        &self,
        reply: Result<Option<&gst::StructureRef>, gst::PromiseError>,
    ) -> Result<(), anyhow::Error> {
        debug!("{reply:?}");
        let reply = match reply {
            Ok(Some(reply)) => reply,
            Ok(None) => {
                bail!("Answer creation future got no response");
            }
            Err(err) => {
                bail!("Answer creation future got error response: {:?}", err);
            }
        };

        if reply.has_field("error") {
            let gerror = reply.value("error").unwrap().get::<GError>();
            println!("{:?}", gerror);
        }

        let a = reply.get::<gst_webrtc::WebRTCSessionDescription>("answer");
        println!("{a:?}");

        let answer = reply
            .value("answer")
            .unwrap()
            .get::<gst_webrtc::WebRTCSessionDescription>()
            .expect("Invalid argument");
        self.webrtcbin
            .emit_by_name::<()>("set-local-description", &[&answer, &None::<gst::Promise>]);

        println!(
            "sending SDP answer to peer: {}",
            answer.sdp().as_text().unwrap()
        );

        let message = serde_json::to_string(&PcMessage::Webrtc(WebrtcMsg::Sdp {
            type_: "answer".to_string(),
            sdp: answer.sdp().as_text().unwrap(),
        }))
        .unwrap();

        self.send_msg_tx
            .lock()
            .unwrap()
            .unbounded_send(WsMessage::Text(message))
            .context("Failed to send SDP answer")?;

        Ok(())
    }

    // Handle incoming SDP answers from the peer
    fn handle_sdp(&self, type_: &str, sdp: &str) -> Result<(), anyhow::Error> {
        self.pipeline.call_async(|pipeline| {
            pipeline
                .set_state(gst::State::Playing)
                .expect("Error starting pipeline state in response to SDP");
        });

        if type_ == "answer" {
            print!("Received answer:\n{}\n", sdp);

            let ret = gst_sdp::SDPMessage::parse_buffer(sdp.as_bytes())
                .map_err(|_| anyhow!("Failed to parse SDP answer"))?;
            let answer =
                gst_webrtc::WebRTCSessionDescription::new(gst_webrtc::WebRTCSDPType::Answer, ret);

            self.webrtcbin
                .emit_by_name::<()>("set-remote-description", &[&answer, &None::<gst::Promise>]);

            Ok(())
        } else if type_ == "offer" {
            print!("Received offer:\n{}\n", sdp);

            let ret = gst_sdp::SDPMessage::parse_buffer(sdp.as_bytes())
                .map_err(|_| anyhow!("Failed to parse SDP offer"))?;

            // And then asynchronously start our pipeline and do the next steps. The
            // pipeline needs to be started before we can create an answer
            let app_clone = self.downgrade();
            self.pipeline.call_async(move |_pipeline| {
                let app = upgrade_weak!(app_clone);

                let offer = gst_webrtc::WebRTCSessionDescription::new(
                    gst_webrtc::WebRTCSDPType::Offer,
                    ret,
                );

                app.0
                    .webrtcbin
                    .emit_by_name::<()>("set-remote-description", &[&offer, &None::<gst::Promise>]);

                let app_clone = app.downgrade();
                let promise = gst::Promise::with_change_func(move |reply| {
                    let app = upgrade_weak!(app_clone);

                    if let Err(err) = app.on_answer_created(reply) {
                        gst::element_error!(
                            app.pipeline,
                            gst::LibraryError::Failed,
                            ("Failed to send SDP answer: {:?}", err)
                        );
                    }
                });

                app.0
                    .webrtcbin
                    .emit_by_name::<()>("create-answer", &[&None::<gst::Structure>, &promise]);
            });

            Ok(())
        } else {
            bail!("Sdp type is not \"answer\" but \"{}\"", type_)
        }
    }

    // Handle incoming ICE candidates from the peer by passing them to webrtcbin
    fn handle_ice(&self, sdp_mline_index: u32, candidate: &str) -> Result<(), anyhow::Error> {
        self.webrtcbin
            .emit_by_name::<()>("add-ice-candidate", &[&sdp_mline_index, &candidate]);

        Ok(())
    }

    // Asynchronously send ICE candidates to the peer via the WebSocket connection as a JSON
    // message
    fn on_ice_candidate(&self, mlineindex: u32, candidate: &str) -> Result<(), anyhow::Error> {
        let message = serde_json::to_string(&PcMessage::Webrtc(WebrtcMsg::Ice {
            candidate: candidate.to_string(),
            sdp_mline_index: mlineindex,
        }))
        .unwrap();

        self.send_msg_tx
            .lock()
            .unwrap()
            .unbounded_send(WsMessage::Text(message))
            .context("Failed to send ICE candidate")?;

        Ok(())
    }

    // Whenever there's a new incoming, encoded stream from the peer create a new decodebin
    fn on_incoming_stream(&self, pad: &gst::Pad) -> Result<(), anyhow::Error> {
        // Early return for the source pads we're adding ourselves
        if pad.direction() != gst::PadDirection::Src {
            return Ok(());
        }

        let decodebin = gst::ElementFactory::make("decodebin").build().unwrap();
        let app_clone = self.downgrade();
        decodebin.connect_pad_added(move |_decodebin, pad| {
            let app = upgrade_weak!(app_clone);

            if let Err(err) = app.on_incoming_decodebin_stream(pad) {
                gst::element_error!(
                    app.pipeline,
                    gst::LibraryError::Failed,
                    ("Failed to handle decoded stream: {:?}", err)
                );
            }
        });

        self.pipeline.add(&decodebin).unwrap();
        decodebin.sync_state_with_parent().unwrap();

        let sinkpad = decodebin.static_pad("sink").unwrap();
        pad.link(&sinkpad).unwrap();

        Ok(())
    }

    // Handle a newly decoded decodebin stream and depending on its type, create the relevant
    // elements or simply ignore it
    fn on_incoming_decodebin_stream(&self, pad: &gst::Pad) -> Result<(), anyhow::Error> {
        let caps = pad.current_caps().unwrap();
        let name = caps.structure(0).unwrap().name();

        let sink = if name.starts_with("video/") {
            gst::parse_bin_from_description(
                "queue ! videoconvert ! videoscale ! autovideosink",
                true,
            )?
        } else if name.starts_with("audio/") {
            gst::parse_bin_from_description(
                "queue ! audioconvert ! audioresample ! autoaudiosink",
                true,
            )?
        } else {
            println!("Unknown pad {:?}, ignoring", pad);
            return Ok(());
        };

        self.pipeline.add(&sink).unwrap();
        sink.sync_state_with_parent()
            .with_context(|| format!("can't start sink for stream {:?}", caps))?;

        let sinkpad = sink.static_pad("sink").unwrap();
        pad.link(&sinkpad)
            .with_context(|| format!("can't link sink for stream {:?}", caps))?;

        Ok(())
    }
}

// Make sure to shut down the pipeline when it goes out of scope
// to release any system resources
impl Drop for AppInner {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}
