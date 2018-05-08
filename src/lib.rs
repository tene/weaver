#[macro_use]
extern crate serde_derive;
extern crate serde;

use std::path::PathBuf;

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub enum ClientRequest {
    RunCommand(String),
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub struct ClientMessage {
    pub id: u32,
    pub request: ClientRequest,
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub enum ServerNotice {
    CommandStarted(u32, String),
    CommandOutput(u32, String),
    CommandCompleted(u32, i32),
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub struct ServerMessage {
    pub id: u32,
    pub notice: ServerNotice,
}

pub fn weaver_socket_path() -> PathBuf {
    let mut socketpath = std::env::home_dir().unwrap();
    socketpath.push(".weaver.socket");
    socketpath
}
