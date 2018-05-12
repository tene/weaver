extern crate futures;
extern crate rmp_serde;
extern crate tokio;
extern crate tokio_io;
extern crate tokio_serde_msgpack;
extern crate tokio_threadpool;
extern crate tokio_uds;
extern crate weaver;

use futures::future::poll_fn;
use futures::sync::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures::AsyncSink;

use tokio::prelude::{task, Async, Future, Sink, Stream};
use tokio_serde_msgpack::{from_io, MsgPackReader, MsgPackWriter};
use tokio_threadpool::blocking;
use tokio_uds::{UnixListener, UnixStream};

use std::collections::HashMap;
use std::process::Command;
use std::sync::{Arc, RwLock};

use weaver::{
    weaver_socket_path, ClientMessage, ClientRequest, CommandHistory, ServerMessage, ServerNotice,
};

type ClientID = u32;

pub struct ServerState {
    pub channels: HashMap<ClientID, UnboundedSender<ServerMessage>>,
    pub command_history: CommandHistory,
}

impl ServerState {
    pub fn new() -> Self {
        let channels = HashMap::new();
        let command_history = CommandHistory::new();
        ServerState {
            channels,
            command_history,
        }
    }
}

pub struct ClientConn<'a> {
    id: ClientID,
    broadcast: UnboundedSender<ServerMessage>,
    pub chan_send: UnboundedSender<ServerMessage>,
    chan_recv: UnboundedReceiver<ServerMessage>,
    socket_tx: MsgPackWriter<UnixStream, ServerMessage>,
    socket_rx: MsgPackReader<'a, UnixStream, ClientMessage>,
    state: Arc<RwLock<ServerState>>,
    overflow: Option<ServerMessage>,
}

fn send_notice(chan: &UnboundedSender<ServerMessage>, id: u32, notice: ServerNotice) {
    let msg = ServerMessage { id, notice };
    let _ = chan.unbounded_send(msg);
}

impl<'a> ClientConn<'a> {
    pub fn new(
        id: ClientID,
        socket: UnixStream,
        state: Arc<RwLock<ServerState>>,
        broadcast: UnboundedSender<ServerMessage>,
    ) -> Self {
        let overflow = None;
        let (socket_rx, socket_tx): (
            MsgPackReader<UnixStream, ClientMessage>,
            MsgPackWriter<UnixStream, ServerMessage>,
        ) = from_io(socket);
        let (chan_send, chan_recv): (
            UnboundedSender<ServerMessage>,
            UnboundedReceiver<ServerMessage>,
        ) = unbounded();

        state
            .write()
            .unwrap()
            .channels
            .insert(id, chan_send.clone());

        ClientConn {
            id,
            state,
            broadcast,
            chan_send,
            chan_recv,
            socket_rx,
            socket_tx,
            overflow,
        }
    }
    pub fn handle_msg(&mut self, _msg: ClientMessage) {}
}

impl<'a> Future for ClientConn<'a> {
    type Item = ();
    type Error = ();
    fn poll(&mut self) -> Result<Async<Self::Item>, Self::Error> {
        if let Some(msg) = self.overflow.take() {
            if let Ok(AsyncSink::NotReady(msg)) = self.socket_tx.start_send(msg) {
                self.overflow = Some(msg);
            }
        }

        if self.overflow == None {
            const LINES_PER_TICK: usize = 10;
            for i in 0..LINES_PER_TICK {
                match self.chan_recv.poll().unwrap() {
                    Async::Ready(Some(msg)) => {
                        if let Ok(AsyncSink::NotReady(msg)) = self.socket_tx.start_send(msg) {
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

        let _ = self.socket_tx.poll_complete();

        while let Async::Ready(msg) = self.socket_rx.poll().unwrap() {
            if let Some(msg) = msg {
                println!("{:#?}", msg);
                let req_id = msg.id;
                let broadcast = self.broadcast.clone();
                match msg.request {
                    ClientRequest::RunCommand(c) => {
                        let cmd_idx = self.state.write().unwrap().command_history.next_index();
                        send_notice(
                            &broadcast,
                            req_id,
                            ServerNotice::CommandStarted(cmd_idx, c.clone()),
                        );
                        let run_command = poll_fn(move || {
                            blocking(|| {
                                Command::new("bash")
                                    .arg("-c")
                                    .arg(&c)
                                    .output()
                                    .expect("ERR: Failed to run command??")
                            }).map_err(|e| panic!("Threadpool Problem: {:#?}", e))
                        }).and_then(move |output| {
                            send_notice(
                                &broadcast,
                                req_id,
                                ServerNotice::CommandOutput(
                                    cmd_idx,
                                    String::from_utf8_lossy(&output.stdout).to_string(),
                                ),
                            );
                            send_notice(
                                &broadcast,
                                req_id,
                                ServerNotice::CommandErr(
                                    cmd_idx,
                                    String::from_utf8_lossy(&output.stderr).to_string(),
                                ),
                            );
                            send_notice(
                                &broadcast,
                                req_id,
                                ServerNotice::CommandCompleted(
                                    cmd_idx,
                                    output.status.code().unwrap(),
                                ),
                            );
                            Ok(())
                        });
                        tokio::spawn(run_command);
                    }
                }
            } else {
                return Ok(Async::Ready(()));
            }
        }
        Ok(Async::NotReady)
    }
}

impl<'a> Drop for ClientConn<'a> {
    fn drop(&mut self) {
        self.state.write().unwrap().channels.remove(&self.id);
    }
}

pub struct WeaverServer {
    state: Arc<RwLock<ServerState>>,
    broadcast_recv: UnboundedReceiver<ServerMessage>,
    broadcast_send: UnboundedSender<ServerMessage>,
    listener: UnixListener,
    next_client_id: ClientID,
}

impl WeaverServer {
    pub fn new() -> Self {
        let socketpath = weaver_socket_path();
        let _ = std::fs::remove_file(&socketpath);

        let state = Arc::new(RwLock::new(ServerState::new()));
        let (broadcast_send, broadcast_recv): (
            UnboundedSender<ServerMessage>,
            UnboundedReceiver<ServerMessage>,
        ) = unbounded();
        let listener = UnixListener::bind(socketpath).unwrap();
        let next_client_id = 1;

        WeaverServer {
            state,
            broadcast_recv,
            broadcast_send,
            listener,
            next_client_id,
        }
    }

    pub fn next_client_id(&mut self) -> ClientID {
        let rv = self.next_client_id;
        self.next_client_id += 1;
        rv
    }
}

impl Future for WeaverServer {
    type Item = ();
    type Error = ();
    fn poll(&mut self) -> Result<Async<Self::Item>, Self::Error> {
        const LINES_PER_TICK: usize = 10;
        for i in 0..LINES_PER_TICK {
            match self.listener.poll_accept().unwrap() {
                Async::Ready((socket, _addr)) => {
                    let client = ClientConn::new(
                        self.next_client_id(),
                        socket,
                        self.state.clone(),
                        self.broadcast_send.clone(),
                    );
                    send_notice(
                        &client.chan_send,
                        0,
                        ServerNotice::CommandsBulk(
                            self.state
                                .read()
                                .unwrap()
                                .command_history
                                .clone()
                                .into_iter()
                                .collect(),
                        ),
                    );
                    tokio::spawn(client);
                    if i + 1 == LINES_PER_TICK {
                        task::current().notify();
                    }
                }
                _ => break,
            }
        }

        for i in 0..LINES_PER_TICK {
            match self.broadcast_recv.poll().unwrap() {
                Async::Ready(Some(msg)) => {
                    let mut state = self.state.write().unwrap();
                    state.command_history.do_update(msg.clone());
                    for (_id, chan) in &state.channels {
                        chan.unbounded_send(msg.clone()).unwrap();
                    }
                    if i + 1 == LINES_PER_TICK {
                        task::current().notify();
                    }
                }
                _ => break,
            }
        }

        Ok(Async::NotReady)
    }
}

fn main() {
    let server = WeaverServer::new();
    tokio::run(server);
    println!("Hello, world!");
}
