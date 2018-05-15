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

fn render_command_summary(cmd: &WeaverCommand, width: usize, maxlines: usize) -> Pane {
    let mut pane = Pane::new_width(width);
    let (icon, style) = match cmd.status {
        None => ('…', "command.running"),
        Some(0) => ('✔', "command.success"),
        _ => ('X', "command.failed"),
    };
    let status_pane = Pane::new_styled(
        Position::new(0, 0),
        Size::new(1, 1),
        vec![icon.to_string()],
        style,
    );
    pane.push_child(status_pane);
    let subwidth = width - 1;
    let command_line = text_to_lines(cmd.cmd.clone(), subwidth);
    let pos = Position::new(1, 0);
    let textlen = command_line.len();
    let mut offset = textlen;
    let size = Size::new(subwidth as u16, textlen as u16);
    pane.push_child(Pane::new_styled(pos, size, command_line, "command"));
    if cmd.stdout.len() > 0 {
        let mut stdout = text_to_lines(cmd.stdout.clone(), subwidth);
        let pos = Position::new(1, offset as u16);
        let mut textlen = stdout.len();
        if textlen > maxlines {
            stdout = stdout.split_off(textlen - maxlines);
            textlen = maxlines;
        }
        offset += textlen;
        let size = Size::new(subwidth as u16, textlen as u16);
        pane.push_child(Pane::new_styled(pos, size, stdout, "stdout"));
    }
    if cmd.stderr.len() > 0 {
        let mut stderr = text_to_lines(cmd.stderr.clone(), subwidth);
        let pos = Position::new(1, offset as u16);
        let mut textlen = stderr.len();
        if textlen > maxlines {
            stderr = stderr.split_off(textlen - maxlines);
            textlen = maxlines;
        }
        let size = Size::new(subwidth as u16, textlen as u16);
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
            let mut child = render_command_summary(cmd, size.width as usize, 10);
            let offset = child.size.height as usize;

            ctr += offset;
            if ctr >= height {
                let spillover = (ctr - height) as u16;
                let clip_pos = Position::new(0, spillover);
                let clip_size = Size::new(size.width, child.size.height - spillover);
                child = child.clip(clip_pos, clip_size).unwrap();
                child.position = Position::new(0, 0);
                ctr = height;
            }
            let child = child.offset(Position::new(0, (height - ctr) as u16));
            children.push(child);
            if ctr == height {
                break;
            }
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
            Key::Alt('\r') => self.input.write().unwrap().process_key(Key::Char('\n')),
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
            "command.running" => (
                Some(Box::new(color::LightYellow)),
                Some(Box::new(color::Rgb(16, 16, 32))),
            ),
            "command.success" => (
                Some(Box::new(color::LightGreen)),
                Some(Box::new(color::Rgb(16, 16, 32))),
            ),
            "command.failed" => (Some(Box::new(color::LightRed)), None),
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
