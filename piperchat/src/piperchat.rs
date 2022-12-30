use serde::{Deserialize, Serialize};

pub mod window;

pub const APP_ID: &str = "eu.mguzik.piperchat";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Message {
    Connect(String),
    WebRtc(WebrtcMsg),
}

// JSON messages we communicate with
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WebrtcMsg {
    Ice {
        candidate: String,
        #[serde(rename = "sdpMLineIndex")]
        sdp_mline_index: u32,
    },
    Sdp {
        #[serde(rename = "type")]
        type_: String,
        sdp: String,
    },
}
