#[macro_use]
extern crate serde_derive;
extern crate serde;

use std::collections::BTreeMap;

#[derive(Debug, Deserialize, Serialize)]
pub struct CommandHistory(BTreeMap<usize, WeaverCommand>);

impl CommandHistory {
    pub fn do_update(&mut self, msg: ServerMessage) {
        use ServerNotice::*;
        match msg.notice {
            CommandStarted(i, cmd) => {
                let _ = self.0.insert(i, WeaverCommand::new(cmd));
            }
            CommandOutput(i, text) => self.0.get_mut(&i).unwrap().stdout.push_str(&text),
            CommandErr(i, text) => self.0.get_mut(&i).unwrap().stderr.push_str(&text),
            CommandCompleted(i, rv) => self.0.get_mut(&i).unwrap().status = Some(rv),
        };
    }

    pub fn new() -> Self {
        CommandHistory(BTreeMap::new())
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (&usize, &WeaverCommand)> {
        self.0.iter()
    }
}

use std::path::PathBuf;

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub struct WeaverCommand {
    pub cmd: String,
    pub stdout: String,
    pub stderr: String,
    pub status: Option<i32>,
}

impl WeaverCommand {
    pub fn new(cmd: String) -> Self {
        WeaverCommand {
            cmd,
            stdout: String::new(),
            stderr: String::new(),
            status: None,
        }
    }
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub enum ClientRequest {
    RunCommand(String),
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub struct ClientMessage {
    pub id: u32,
    pub request: ClientRequest,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub enum ServerNotice {
    CommandStarted(usize, String),
    CommandOutput(usize, String),
    CommandErr(usize, String),
    CommandCompleted(usize, i32),
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct ServerMessage {
    pub id: u32,
    pub notice: ServerNotice,
}

pub fn weaver_socket_path() -> PathBuf {
    let mut socketpath = std::env::home_dir().unwrap();
    socketpath.push(".weaver.socket");
    socketpath
}
