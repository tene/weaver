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

use std::sync::mpsc::Sender;
use std::thread;

use futures::sync::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};

use tokio::prelude::{Future, Sink, Stream};
use tokio_serde_msgpack::{from_io, MsgPackReader, MsgPackWriter};
use tokio_uds::UnixStream;

use weaver::{weaver_socket_path, ClientMessage, ServerMessage};

#[derive(Debug, PartialEq, Clone)]
pub enum WeaverNotification {
    _Heartbeat,
}

pub struct WeaverClient {
    pub notifications: Sender<WeaverNotification>,
    pub commands: UnboundedSender<ClientMessage>,
}

impl WeaverClient {
    pub fn new(notifications: Sender<WeaverNotification>) -> Self {
        let socketpath = weaver_socket_path();
        let socket = UnixStream::connect(socketpath).unwrap();

        let (socket_rx, socket_tx): (
            MsgPackReader<UnixStream, ServerMessage>,
            MsgPackWriter<UnixStream, ClientMessage>,
        ) = from_io(socket);
        let socket_tx = socket_tx.sink_map_err(|e| println!("Send Err: {:#?}", e));

        let (commands, commands_tx): (
            UnboundedSender<ClientMessage>,
            UnboundedReceiver<ClientMessage>,
        ) = unbounded();

        let session = commands_tx
            .forward(socket_tx)
            .map_err(|e| println!("Encode Err: {:#?}", e))
            .and_then(|_| Ok(()));

        thread::spawn(move || {
            tokio::run(session);
        });
        WeaverClient {
            notifications,
            commands,
        }
    }

    pub fn send_cmd(&mut self, cmd: ClientMessage) {
        self.commands.unbounded_send(cmd);
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
        let lines = text.lines();
        let mut log = self.log.write().unwrap();
        for line in lines {
            self.weaver.send_cmd(ClientMessage {
                id: 5,
                name: line.to_owned(),
            });
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
        self.log_msg(&format!("{:?}", event));
        match event {
            Event::InputEvent(i) => match i {
                Input::Key(Key::Esc) => Err(None),
                Input::Key(k) => {
                    self.input(k);
                    Ok(())
                }
                _ => Ok(()),
            },
            Event::AppEvent(_) => Ok(()),
        }
    }
}

fn main() {
    let mut be = Backend::new();
    let sender = be.sender.clone();
    let weaver = WeaverClient::new(sender);
    let mut app = WeaverTui::new(weaver);
    app.log_msg("Esc to exit");
    be.run_app(&mut app);
}
