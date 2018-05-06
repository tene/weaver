#[macro_use]
extern crate serde_derive;
extern crate serde;

use std::path::PathBuf;

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub struct ClientMessage {
    pub id: u32,
    pub name: String,
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub struct ServerMessage {
    pub id: u32,
    pub name: String,
}

pub fn weaver_socket_path() -> PathBuf {
    let mut socketpath = std::env::home_dir().unwrap();
    socketpath.push(".weaver.socket");
    socketpath
}
