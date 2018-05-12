#[macro_use]
extern crate serde_derive;
extern crate futures;
extern crate serde;
extern crate tokio;
extern crate tokio_serde_msgpack;
extern crate tokio_uds;

use std::collections::BTreeMap;
use std::iter::FromIterator;

pub mod client;
pub use client::{WeaverClient, WeaverNotification, WeaverState};

pub type CommandId = u32;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CommandHistory {
    pub commands: BTreeMap<CommandId, WeaverCommand>,
    next_index: CommandId,
}

impl CommandHistory {
    pub fn do_update(&mut self, msg: ServerMessage) {
        use ServerNotice::*;
        match msg.notice {
            CommandStarted(i, cmd) => {
                let _ = self.commands.insert(i, WeaverCommand::new(cmd));
            }
            CommandOutput(i, text) => self.commands.get_mut(&i).unwrap().stdout.push_str(&text),
            CommandErr(i, text) => self.commands.get_mut(&i).unwrap().stderr.push_str(&text),
            CommandCompleted(i, rv) => self.commands.get_mut(&i).unwrap().status = Some(rv),
            CommandsBulk(cmds) => {
                let mut bulk = BTreeMap::from_iter(cmds.into_iter());
                self.commands.append(&mut bulk);
            }
        };
    }

    pub fn new() -> Self {
        let commands = BTreeMap::new();
        let next_index = 1;
        CommandHistory {
            commands,
            next_index,
        }
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (&CommandId, &WeaverCommand)> {
        self.commands.iter()
    }

    pub fn into_iter(self) -> impl Iterator<Item = (CommandId, WeaverCommand)> {
        self.commands.into_iter()
    }

    pub fn next_index(&mut self) -> CommandId {
        let rv = self.next_index;
        self.next_index += 1;
        rv
    }
}

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
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
    CommandsBulk(Vec<(CommandId, WeaverCommand)>),
    CommandStarted(CommandId, String),
    CommandOutput(CommandId, String),
    CommandErr(CommandId, String),
    CommandCompleted(CommandId, i32),
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
