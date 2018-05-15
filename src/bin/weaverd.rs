extern crate futures;
extern crate rmp_serde;
extern crate tokio;
extern crate tokio_io;
//extern crate tokio_process;
extern crate tokio_serde_msgpack;
extern crate tokio_threadpool;
extern crate tokio_uds;
extern crate weaver;

use futures::sync::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures::AsyncSink;

use tokio::prelude::{task, Async, AsyncRead, Future, Sink, Stream};
use tokio_serde_msgpack::{from_io, MsgPackReader, MsgPackWriter};
use tokio_uds::{UnixListener, UnixStream};

use std::collections::HashMap;
use std::io::BufReader;
use std::process::{Command, Stdio};
use std::sync::{Arc, RwLock};

use weaver::process::{stdio, Child, ChildStderr, ChildStdout};
use weaver::{
    weaver_socket_path, ClientMessage, ClientRequest, CommandHistory, CommandId, ServerMessage,
    ServerNotice,
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
    println!("{:#?}", notice);
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
                        let mut cmd = Command::new("bash");
                        cmd.arg("-c").arg(&c);
                        send_notice(&broadcast, req_id, ServerNotice::CommandStarted(cmd_idx, c));

                        let run_command = RunningCommand::new(cmd, broadcast, req_id, cmd_idx);

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

// XXX Maybe refactor this into Stream<ServerMessage>?
pub struct RunningCommand {
    child: Child,
    broadcast: UnboundedSender<ServerMessage>,
    stdout: BufReader<ChildStdout>,
    stderr: BufReader<ChildStderr>,
    buf: Vec<u8>,
    request_id: u32,
    command_id: CommandId,
}

// XXX Use tokio-process once fixed: https://github.com/alexcrichton/tokio-process/issues/29
impl RunningCommand {
    pub fn new(
        mut cmd: Command,
        broadcast: UnboundedSender<ServerMessage>,
        request_id: u32,
        command_id: CommandId,
    ) -> Self {
        let mut child = cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Failed to execute child process");

        let stdout = child.stdout.take().unwrap();
        let stdout = stdio(stdout).unwrap();
        let stdout = BufReader::new(stdout);

        let stderr = child.stderr.take().unwrap();
        let stderr = stdio(stderr).unwrap();
        let stderr = BufReader::new(stderr);

        let child = Child::new(child);

        const CHUNK_SIZE: usize = 1024;
        let buf = vec![0; CHUNK_SIZE];
        RunningCommand {
            child,
            broadcast,
            stdout,
            stderr,
            buf,
            request_id,
            command_id,
        }
    }
}

impl Future for RunningCommand {
    type Item = ();
    type Error = ();
    fn poll(&mut self) -> Result<Async<Self::Item>, Self::Error> {
        const CHUNKS_PER_TICK: usize = 10;
        for i in 0..CHUNKS_PER_TICK {
            match self.stdout.poll_read(&mut self.buf).unwrap() {
                Async::Ready(size) => {
                    if size > 0 {
                        let text = String::from_utf8_lossy(&self.buf[..size]).to_string();
                        send_notice(
                            &self.broadcast,
                            self.request_id,
                            ServerNotice::CommandOutput(self.command_id, text),
                        );
                    }
                    if i + 1 == CHUNKS_PER_TICK {
                        task::current().notify();
                    }
                }
                _ => break,
            }
        }

        for i in 0..CHUNKS_PER_TICK {
            match self.stderr.poll_read(&mut self.buf).unwrap() {
                Async::Ready(size) => {
                    if size > 0 {
                        let text = String::from_utf8_lossy(&self.buf[..size]).to_string();
                        send_notice(
                            &self.broadcast,
                            self.request_id,
                            ServerNotice::CommandErr(self.command_id, text),
                        );
                    }
                    if i + 1 == CHUNKS_PER_TICK {
                        task::current().notify();
                    }
                }
                _ => break,
            }
        }

        return match self.child.poll().unwrap() {
            Async::Ready(status) => {
                send_notice(
                    &self.broadcast,
                    self.request_id,
                    ServerNotice::CommandCompleted(self.command_id, status.code().unwrap()),
                );
                Ok(Async::Ready(()))
            }
            _ => Ok(Async::NotReady),
        };
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
