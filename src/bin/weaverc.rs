extern crate futures;
extern crate text_ui;
extern crate tokio;
extern crate tokio_serde_msgpack;
extern crate tokio_uds;
extern crate weaver;

use text_ui::app::App;
use text_ui::backend::Backend;
use text_ui::widget::{shared, Linear, Shared, Text, TextInput};
use text_ui::{Event, Input, Key};

use std::sync::mpsc::SendError as StdSendError;
use std::sync::mpsc::Sender;
use std::sync::{Arc, RwLock};
use std::thread;

use futures::sync::mpsc::SendError as FutureSendError;
use futures::sync::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};

use tokio::prelude::{Future, Sink, Stream};
use tokio_serde_msgpack::{from_io, MsgPackReader, MsgPackWriter};
use tokio_uds::UnixStream;

use weaver::{weaver_socket_path, ClientMessage, ClientRequest, ServerMessage, WeaverCommand};

#[derive(Debug, PartialEq)]
pub enum WeaverNotification {
    _Heartbeat,
    Server(ServerMessage),
}

struct WeaverClientCore {
    pub notifications: Sender<WeaverNotification>,
    pub commands: Vec<WeaverCommand>,
}

impl WeaverClientCore {
    fn new(notifications: Sender<WeaverNotification>) -> Arc<RwLock<Self>> {
        let commands = vec![];
        Arc::new(RwLock::new(WeaverClientCore {
            notifications,
            commands,
        }))
    }

    fn _send_notification(
        &self,
        n: WeaverNotification,
    ) -> Result<(), StdSendError<WeaverNotification>> {
        self.notifications.send(n)
    }
}

// XXX Need to refactor this into impl Future
pub struct WeaverClient {
    pub commands: UnboundedSender<ClientMessage>,
    _core: Arc<RwLock<WeaverClientCore>>,
    msgcounter: u32,
}

impl WeaverClient {
    pub fn new(notifications: Sender<WeaverNotification>) -> Self {
        let (commands, commands_tx): (
            UnboundedSender<ClientMessage>,
            UnboundedReceiver<ClientMessage>,
        ) = unbounded();

        // XXX Can't share Sender between threads
        let notifications2 = notifications.clone();
        let msgcounter = 0;

        let core = WeaverClientCore::new(notifications);
        let core2 = core.clone();
        let socketpath = weaver_socket_path();
        let socketf = futures::future::result(UnixStream::connect(socketpath));

        let run_client_socket = socketf
            .and_then(|socket| {
                let (socket_rx, socket_tx): (
                    MsgPackReader<UnixStream, ServerMessage>,
                    MsgPackWriter<UnixStream, ClientMessage>,
                ) = from_io(socket);
                let socket_tx = socket_tx.sink_map_err(|e| println!("Send Err: {:#?}", e));
                let socket_rx = socket_rx.map_err(|e| panic!("Decode Error: {:#?}", e));

                let send_commands = commands_tx
                    .forward(socket_tx)
                    .map_err(|e| println!("Encode Err: {:#?}", e))
                    .and_then(|_| Ok(()));

                tokio::spawn(send_commands);

                let recv_msgs = socket_rx
                    .for_each(move |msg| notifications2.send(WeaverNotification::Server(msg)))
                    .and_then(|_| Ok(()))
                    .map_err(|e| panic!("Notification Send Error: {:#?}", e));

                tokio::spawn(recv_msgs);

                Ok(())
            })
            .map_err(|e| panic!("Socket Connect Error: {:#?}", e));

        thread::spawn(move || {
            tokio::run(run_client_socket);
        });
        WeaverClient {
            commands,
            _core: core2,
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
        self.commands.unbounded_send(msg)
    }
}

struct WeaverTui {
    log: Shared<Text>,
    input: Shared<TextInput>,
    vbox: Shared<Linear>,
    weaver: WeaverClient,
}

impl WeaverTui {
    fn new(weaver: WeaverClient) -> WeaverTui {
        let log = shared(Text::new(vec![]));
        let input = shared(TextInput::new(""));
        let mut mainbox = Linear::vbox();
        mainbox.push(&log);
        mainbox.push(&input);
        let vbox = shared(mainbox);
        WeaverTui {
            log,
            input,
            vbox,
            weaver,
        }
    }

    fn submit_input(&mut self) {
        let text = self.input.write().unwrap().submit();
        self.weaver.run_command(text.clone()).unwrap();
        let lines = text.lines();
        let mut log = self.log.write().unwrap();
        for line in lines {
            log.push(line.to_owned());
        }
    }

    fn log_msg(&mut self, msg: &str) {
        let lines: Vec<String> = msg.lines().map(|l| l.to_owned()).collect();
        self.log.write().unwrap().lines.extend(lines);
    }

    fn input(&mut self, key: Key) {
        match key {
            Key::Char('\n') => self.submit_input(),
            k => self.input.write().unwrap().keypress(k),
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
    let mut app = WeaverTui::new(weaver);
    app.log_msg("Esc to exit");
    be.run_app(&mut app);
}
