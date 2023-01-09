use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "lowercase")]
pub enum Message {
    Connect(ConnectMessage),
    ConnectResponse(ConnectResponse),
    UserList(UserList),
    Webrtc(WebrtcMsg),
    Call(CallMessage),
    CallReceived(CallReceivedMessage),
    CallHangup,
    CallResponse(CallResponseMessage),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConnectMessage {
    pub name: String,
    pub id: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ConnectResponse {
    Accept,
    Reject(String),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UserList {
    pub users: Vec<(usize, String)>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CallMessage {
    pub peer: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum CallResponseMessage {
    Accept,
    Reject,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CallReceivedMessage {
    pub name: String,
}

// JSON messages we communicate with
#[derive(Serialize, Deserialize, Debug, Clone)]
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
