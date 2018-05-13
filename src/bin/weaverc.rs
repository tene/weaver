extern crate futures;
extern crate text_ui;
extern crate tokio;
extern crate tokio_serde_msgpack;
extern crate tokio_uds;
extern crate weaver;

use text_ui::app::App;
use text_ui::backend::{color, Backend, Color};
use text_ui::pane::Pane;
//use text_ui::widget::DbgDump;
use text_ui::widget::Widget;
use text_ui::widget::{shared, Line, Linear, Readline, Shared, Text};
use text_ui::{text_to_lines, Event, Input, Key, Position, Size};

use std::sync::{Arc, RwLock};
use std::thread;

use tokio::prelude::Future;
use weaver::{WeaverClient, WeaverCommand, WeaverNotification, WeaverState};

struct WeaverStateWidget {
    state: Shared<WeaverState>,
}

fn render_command_summary(cmd: &WeaverCommand, width: usize) -> Pane {
    let mut pane = Pane::new_width(width);
    let mut offset: usize = 0;
    let status = match cmd.status {
        None => '…',
        Some(0) => '✔',
        _ => '❌',
    };
    let command_line = format!("{} {}", status, cmd.cmd.clone());
    let command_line = text_to_lines(command_line, width as usize);
    let pos = Position::new(0, offset as u16);
    let textlen = command_line.len();
    offset += textlen;
    let size = Size::new(width as u16, textlen as u16);
    pane.push_child(Pane::new_styled(pos, size, command_line, "command"));
    if cmd.stdout.len() > 0 {
        let stdout = text_to_lines(cmd.stdout.clone(), size.width as usize);
        let pos = Position::new(0, offset as u16);
        let textlen = stdout.len();
        offset += textlen;
        let size = Size::new(width as u16, textlen as u16);
        pane.push_child(Pane::new_styled(pos, size, stdout, "stdout"));
    }
    if cmd.stderr.len() > 0 {
        let stderr = text_to_lines(cmd.stderr.clone(), size.width as usize);
        let pos = Position::new(0, offset as u16);
        let textlen = stderr.len();
        let size = Size::new(width as u16, textlen as u16);
        pane.push_child(Pane::new_styled(pos, size, stderr, "stderr"));
    }
    pane
}

impl Widget for WeaverStateWidget {
    fn render_children(&self, size: Size) -> Option<Vec<Pane>> {
        let height = size.height as usize;
        let mut ctr: usize = 0;
        let state = self.state.read().unwrap();
        let mut children: Vec<Pane> = vec![];
        for (_i, cmd) in state.command_history.iter().rev() {
            let child = render_command_summary(cmd, size.width as usize);
            let offset = child.size.height as usize;

            ctr += offset;
            if ctr >= height {
                // XXX Need to be able to clip the top of the overflow
                break;
            }
            let child = child.offset(Position::new(0, (height - ctr) as u16));
            children.push(child);
        }
        Some(children)
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
    fn style(&self, name: &str) -> (Option<Box<Color>>, Option<Box<Color>>) {
        match name {
            "command" => (None, Some(Box::new(color::Rgb(16, 16, 32)))),
            "stderr" => (None, Some(Box::new(color::Rgb(32, 16, 16)))),
            "input" => (None, Some(Box::new(color::Rgb(32, 32, 32)))),
            _ => (None, None),
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
