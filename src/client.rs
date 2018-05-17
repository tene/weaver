use std::fmt;
use std::io;
use std::sync::mpsc::Sender;
use std::sync::{Arc, RwLock};

use futures::sync::mpsc::SendError as FutureSendError;
use futures::sync::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures::AsyncSink;

use tokio::prelude::{task, Async, Future, Sink, Stream};
use tokio_serde_msgpack::{from_io, DecodeError, MsgPackReader, MsgPackWriter};
use tokio_uds::UnixStream;

use super::{weaver_socket_path, ClientMessage, ClientRequest, CommandHistory, ServerMessage};

#[derive(Debug, PartialEq)]
pub enum WeaverNotification {
    Updated,
    Server(ServerMessage),
}

pub struct WeaverState {
    pub command_history: CommandHistory,
    pub commands_tx: UnboundedSender<ClientMessage>,
    msgcounter: u32,
}

impl fmt::Debug for WeaverState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("WeaverState")
            .field("command_history", &self.command_history)
            .finish()
    }
}

impl WeaverState {
    pub fn new(commands_tx: UnboundedSender<ClientMessage>) -> Self {
        let command_history = CommandHistory::new();
        let msgcounter = 0;
        WeaverState {
            commands_tx,
            command_history,
            msgcounter,
        }
    }
    pub fn run_command(&mut self, cmd: String) -> Result<(), FutureSendError<ClientMessage>> {
        let request = ClientRequest::RunCommand(cmd);
        self.send_request(request)
    }

    pub fn send_request(
        &mut self,
        request: ClientRequest,
    ) -> Result<(), FutureSendError<ClientMessage>> {
        self.msgcounter += 1;
        let id = self.msgcounter;
        let msg = ClientMessage { id, request };
        self.commands_tx.unbounded_send(msg)
    }

    fn do_update(&mut self, msg: ServerMessage) -> Option<WeaverNotification> {
        self.command_history.do_update(msg);
        Some(WeaverNotification::Updated)
    }
}

pub struct WeaverClient<'a> {
    commands_rx: UnboundedReceiver<ClientMessage>,
    connection: WeaverClientConnectionState<'a>,
    pub state: Arc<RwLock<WeaverState>>,
    pub notifications: Sender<WeaverNotification>,
    overflow: Option<ClientMessage>,
}

impl<'a> WeaverClient<'a> {
    pub fn new(notifications: Sender<WeaverNotification>) -> Self {
        let (commands_tx, commands_rx): (
            UnboundedSender<ClientMessage>,
            UnboundedReceiver<ClientMessage>,
        ) = unbounded();

        let state = Arc::new(RwLock::new(WeaverState::new(commands_tx)));
        let overflow = None;

        let socketpath = weaver_socket_path();
        let socket = UnixStream::connect(socketpath);
        let connection = WeaverClientConnectionState::Pending(Box::new(socket));
        //let socket_tx = socket_tx.sink_map_err(|e| println!("Send Err: {:#?}", e));
        //let socket_rx = socket_rx.map_err(|e| panic!("Decode Error: {:#?}", e));

        WeaverClient {
            commands_rx,
            connection,
            notifications,
            state,
            overflow,
        }
    }

    fn _do_update(&mut self, msg: ServerMessage) -> Option<WeaverNotification> {
        let command_history = &mut self.state.write().unwrap().command_history;
        command_history.do_update(msg);
        Some(WeaverNotification::Updated)
    }
}

enum WeaverClientConnectionState<'a> {
    Pending(Box<Future<Item = UnixStream, Error = io::Error> + Send>),
    Connected(
        MsgPackReader<'a, UnixStream, ServerMessage>,
        MsgPackWriter<UnixStream, ClientMessage>,
    ),
    Failed(io::Error),
}

impl<'a> WeaverClientConnectionState<'a> {
    pub fn try_connect(
        &mut self,
    ) -> Result<
        Async<(
            &mut MsgPackReader<'a, UnixStream, ServerMessage>,
            &mut MsgPackWriter<UnixStream, ClientMessage>,
        )>,
        &io::Error,
    > {
        use self::WeaverClientConnectionState::*;
        match self {
            Pending(socket) => match socket.poll() {
                Ok(Async::Ready(socket)) => {
                    let (mut reader, mut writer): (
                        MsgPackReader<UnixStream, ServerMessage>,
                        MsgPackWriter<UnixStream, ClientMessage>,
                    ) = from_io(socket);
                    *self = Connected(reader, writer);
                    self.try_connect()
                }
                Ok(Async::NotReady) => return Ok(Async::NotReady),
                Err(err) => {
                    *self = Failed(err);
                    self.try_connect()
                }
            },
            Connected(reader, writer) => Ok(Async::Ready((reader, writer))),
            Failed(err) => Err(err),
        }
    }
}

impl<'a> Future for WeaverClient<'a> {
    type Item = ();
    type Error = DecodeError;
    fn poll(&mut self) -> Result<Async<Self::Item>, Self::Error> {
        let connection = &mut self.connection;
        let (socket_rx, socket_tx) = match connection.try_connect() {
            Ok(Async::Ready(pair)) => pair,
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Err(err) => panic!("Could not connect to weaver daemon: {:#?}", err),
        };

        if let Some(msg) = self.overflow.take() {
            if let Ok(AsyncSink::NotReady(msg)) = socket_tx.start_send(msg) {
                self.overflow = Some(msg);
            }
        }

        if self.overflow == None {
            const LINES_PER_TICK: usize = 10;
            for i in 0..LINES_PER_TICK {
                match self.commands_rx.poll().unwrap() {
                    Async::Ready(Some(msg)) => {
                        if let Ok(AsyncSink::NotReady(msg)) = socket_tx.start_send(msg) {
                            self.overflow = Some(msg);
                        }
                        if i + 1 == LINES_PER_TICK {
                            task::current().notify();
                        }
                    }
                    _ => break,
                }
            }
        }

        let _ = socket_tx.poll_complete();

        while let Async::Ready(msg) = socket_rx.poll()? {
            if let Some(msg) = msg {
                self.notifications
                    .send(WeaverNotification::Server(msg.clone()))
                    .unwrap();
                match self.state.write().unwrap().do_update(msg) {
                    Some(notification) => self.notifications.send(notification).unwrap(),
                    None => {}
                };
            } else {
                return Ok(Async::Ready(()));
            }
        }
        Ok(Async::NotReady)
    }
}
