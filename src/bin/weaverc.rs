extern crate futures;
extern crate text_ui;
extern crate tokio;
extern crate tokio_serde_msgpack;
extern crate tokio_uds;
extern crate weaver;

use text_ui::app::App;
use text_ui::backend::Backend;
use text_ui::pane::Pane;
//use text_ui::widget::DbgDump;
use text_ui::widget::Widget;
use text_ui::widget::{shared, Line, Linear, Readline, Shared, Text};
use text_ui::{text_to_lines, Event, Input, Key, Position, Size};

use std::fmt;
use std::sync::mpsc::Sender;
use std::sync::{Arc, RwLock};
use std::thread;

use futures::sync::mpsc::SendError as FutureSendError;
use futures::sync::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures::AsyncSink;

use tokio::prelude::{task, Async, Future, Sink, Stream};
use tokio_serde_msgpack::{from_io, MsgPackReader, MsgPackWriter};
use tokio_uds::UnixStream;

use weaver::{weaver_socket_path, ClientMessage, ClientRequest, CommandHistory, ServerMessage};

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
    pub fn run_command(
        &mut self,
        cmd: String,
    ) -> Result<(), FutureSendError<weaver::ClientMessage>> {
        let request = ClientRequest::RunCommand(cmd);
        self.send_request(request)
    }

    pub fn send_request(
        &mut self,
        request: ClientRequest,
    ) -> Result<(), FutureSendError<weaver::ClientMessage>> {
        self.msgcounter += 1;
        let id = self.msgcounter;
        let msg = ClientMessage { id, request };
        self.commands_tx.unbounded_send(msg)
    }
}

pub struct WeaverClient<'a> {
    commands_rx: UnboundedReceiver<ClientMessage>,
    socket_rx: MsgPackReader<'a, UnixStream, ServerMessage>,
    socket_tx: MsgPackWriter<UnixStream, ClientMessage>,
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
        let (socket_rx, socket_tx): (
            MsgPackReader<UnixStream, ServerMessage>,
            MsgPackWriter<UnixStream, ClientMessage>,
        ) = from_io(socket.expect("Could not connect to weaver daemon"));
        //let socket_tx = socket_tx.sink_map_err(|e| println!("Send Err: {:#?}", e));
        //let socket_rx = socket_rx.map_err(|e| panic!("Decode Error: {:#?}", e));

        WeaverClient {
            commands_rx,
            socket_rx,
            socket_tx,
            notifications,
            state,
            overflow,
        }
    }

    fn do_update(&mut self, msg: ServerMessage) -> Option<WeaverNotification> {
        let command_history = &mut self.state.write().unwrap().command_history;
        command_history.do_update(msg);
        Some(WeaverNotification::Updated)
    }
}

impl<'a> Future for WeaverClient<'a> {
    type Item = ();
    type Error = tokio_serde_msgpack::DecodeError;
    fn poll(&mut self) -> Result<Async<Self::Item>, Self::Error> {
        if let Some(msg) = self.overflow.take() {
            if let Ok(AsyncSink::NotReady(msg)) = self.socket_tx.start_send(msg) {
                self.overflow = Some(msg);
            }
        }

        if self.overflow == None {
            const LINES_PER_TICK: usize = 10;
            for i in 0..LINES_PER_TICK {
                match self.commands_rx.poll().unwrap() {
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

        while let Async::Ready(msg) = self.socket_rx.poll()? {
            if let Some(msg) = msg {
                self.notifications
                    .send(WeaverNotification::Server(msg.clone()))
                    .unwrap();
                match self.do_update(msg) {
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

struct WeaverStateWidget {
    state: Shared<WeaverState>,
}

impl Widget for WeaverStateWidget {
    fn render_children(&self, size: Size) -> Option<Vec<Pane>> {
        let mut rv = vec![];
        let height = size.height as usize;
        let mut ctr: usize = 0;
        let state = self.state.read().unwrap();
        for (_i, cmd) in state.command_history.iter().rev() {
            let status = match cmd.status {
                None => '…',
                Some(0) => '✔',
                _ => '❌',
            };
            let mut content = vec![];
            let command_line = format!("{} {}", status, cmd.cmd.clone());
            let command_line = text_to_lines(command_line, size.width as usize);
            ctr += command_line.len();
            content.extend(command_line);
            if cmd.stdout.len() > 0 {
                let stdout = text_to_lines(cmd.stdout.clone(), size.width as usize);
                ctr += stdout.len();
                content.extend(stdout);
            }
            if ctr >= height {
                let top_spill = ctr - height;
                content = content.split_off(top_spill);
                ctr = height;
            }
            let pos = Position::new(0, size.height - ctr as u16);
            rv.push(Pane::new(pos, content));
            if ctr == height {
                break;
            }
        }
        Some(rv)
    }
}

struct WeaverTui {
    log: Shared<Text>,
    input: Shared<Readline>,
    vbox: Shared<Linear>,
    content: Shared<Linear>,
    state: Shared<WeaverState>,
    statew: Shared<WeaverStateWidget>,
    show_debug: bool,
}

impl WeaverTui {
    fn new(state: Arc<RwLock<WeaverState>>) -> WeaverTui {
        let log = shared(Text::new(vec![]));
        let input = shared(Readline::new());
        let state: Shared<WeaverState> = state.into();
        let statew = shared(WeaverStateWidget {
            state: state.clone(),
        });
        //let dbgdump = shared(DbgDump::new(&state));
        let show_debug = false;
        let mut contentbox = Linear::hbox();
        //contentbox.push(&dbgdump);
        contentbox.push(&statew);
        let content = shared(contentbox);
        let mut mainbox = Linear::vbox();
        mainbox.push(&content);
        mainbox.push(&input);
        let vbox = shared(mainbox);
        WeaverTui {
            log,
            input,
            vbox,
            content,
            state,
            statew,
            show_debug,
        }
    }

    fn toggle_debug(&mut self) {
        let mut content = self.content.write().unwrap();
        match self.show_debug {
            true => {
                self.show_debug = false;
                content.contents.truncate(0);
                content.push(&self.statew);
            }
            false => {
                self.show_debug = true;
                content.contents.truncate(0);
                content.push(&self.statew);
                content.push(&shared(Line::vertical()));
                content.push(&self.log);
            }
        }
    }

    fn submit_input(&mut self) {
        let text = self.input.write().unwrap().finalize();
        self.state
            .write()
            .unwrap()
            .run_command(text.clone())
            .unwrap();
    }

    fn log_msg(&mut self, msg: &str) {
        let lines: Vec<String> = msg.lines().map(|l| l.to_owned()).collect();
        self.log.write().unwrap().lines.extend(lines);
    }

    fn input(&mut self, key: Key) {
        match key {
            Key::Char('\n') => self.submit_input(),
            k => self.input.write().unwrap().process_key(k),
        }
    }
}

impl App for WeaverTui {
    type UI = Shared<Linear>;
    type MyEvent = WeaverNotification;
    fn widget(&self) -> Self::UI {
        self.vbox.clone()
    }
    fn handle_event(&mut self, event: Event<Self::MyEvent>) -> Result<(), Option<String>> {
        match event {
            Event::InputEvent(i) => match i {
                Input::Key(Key::Esc) => Err(None),
                Input::Key(Key::Alt('d')) => {
                    self.toggle_debug();
                    Ok(())
                }
                Input::Key(k) => {
                    self.input(k);
                    Ok(())
                }
                _ => Ok(()),
            },
            Event::AppEvent(_) => {
                self.log_msg(&format!("{:?}", event));
                Ok(())
            }
        }
    }
}

fn main() {
    let be = Backend::new();
    let sender = be.sender.clone();
    let weaver = WeaverClient::new(sender);
    let mut app = WeaverTui::new(weaver.state.clone());
    thread::spawn(move || {
        tokio::run(weaver.map_err(|e| panic!("Client Error: {:#?}", e)));
    });
    app.log_msg("Esc to exit");
    be.run_app(&mut app);
}
