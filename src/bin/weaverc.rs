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
    selected: Option<usize>,
}

impl WeaverStateWidget {
    pub fn new(state: Shared<WeaverState>) -> Self {
        let selected = None;
        WeaverStateWidget { state, selected }
    }

    pub fn find_cmd_by_index(&self, i: usize) -> Option<String> {
        let state = self.state.read().unwrap();
        let rv = state
            .command_history
            .iter()
            .rev()
            .skip(i)
            .next()
            .map(|(_, cmd)| cmd.cmd.clone());
        rv
    }
}

fn render_command_summary(
    cmd: &WeaverCommand,
    width: usize,
    maxlines: usize,
    selected: bool,
) -> Pane {
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
    let size = Size::new(subwidth, textlen);
    let prefix = match selected {
        true => "selected.",
        false => "",
    };
    pane.push_child(Pane::new_styled(
        pos,
        size,
        command_line,
        &format!("{}command", prefix),
    ));
    if cmd.stdout.len() > 0 {
        let mut stdout = text_to_lines(cmd.stdout.clone(), subwidth);
        let pos = Position::new(1, offset);
        let mut textlen = stdout.len();
        if textlen > maxlines {
            stdout = stdout.split_off(textlen - maxlines);
            textlen = maxlines;
        }
        offset += textlen;
        let size = Size::new(subwidth, textlen);
        pane.push_child(Pane::new_styled(
            pos,
            size,
            stdout,
            &format!("{}stdout", prefix),
        ));
    }
    if cmd.stderr.len() > 0 {
        let mut stderr = text_to_lines(cmd.stderr.clone(), subwidth);
        let pos = Position::new(1, offset);
        let mut textlen = stderr.len();
        if textlen > maxlines {
            stderr = stderr.split_off(textlen - maxlines);
            textlen = maxlines;
        }
        let size = Size::new(subwidth, textlen);
        pane.push_child(Pane::new_styled(
            pos,
            size,
            stderr,
            &format!("{}stderr", prefix),
        ));
    }
    pane
}

fn render_command_detail(cmd: &WeaverCommand, size: Size) -> Pane {
    let mut pane = Pane::new_width(size.width);
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
    let subwidth: usize = size.width - 1;
    let command_line = text_to_lines(cmd.cmd.clone(), subwidth);
    let pos = Position::new(1, 0);
    let textlen = command_line.len();
    let mut offset = textlen;
    let pane_size = Size::new(subwidth, textlen);
    let prefix = "selected.";
    let mut maxlines = size.height - textlen;
    pane.push_child(Pane::new_styled(
        pos,
        pane_size,
        command_line,
        &format!("{}command", prefix),
    ));
    if cmd.stdout.len() > 0 {
        let mut stdout = text_to_lines(cmd.stdout.clone(), subwidth);
        let pos = Position::new(1, offset);
        let mut textlen = stdout.len();
        if textlen > maxlines {
            stdout = stdout.split_off(textlen - maxlines);
            textlen = maxlines;
        }
        maxlines -= textlen;
        offset += textlen;
        let pane_size = Size::new(subwidth, textlen);
        pane.push_child(Pane::new_styled(
            pos,
            pane_size,
            stdout,
            &format!("{}stdout", prefix),
        ));
    }
    if cmd.stderr.len() > 0 {
        let mut stderr = text_to_lines(cmd.stderr.clone(), subwidth);
        let pos = Position::new(1, offset);
        let mut textlen = stderr.len();
        if textlen > maxlines {
            stderr = stderr.split_off(textlen - maxlines);
            textlen = maxlines;
        }
        let pane_size = Size::new(subwidth, textlen);
        pane.push_child(Pane::new_styled(
            pos,
            pane_size,
            stderr,
            &format!("{}stderr", prefix),
        ));
    }
    pane
}

impl Widget for WeaverStateWidget {
    fn render_children(&self, size: Size) -> Option<Vec<Pane>> {
        let height = size.height;
        let mut ctr: usize = 0;
        let state = self.state.read().unwrap();
        let mut children: Vec<Pane> = vec![];
        let mut i = 0;
        let child_width: usize = match self.selected {
            None => size.width,
            Some(_) => size.width / 2,
        };
        for (_i, cmd) in state.command_history.iter().rev() {
            let selected: bool = match self.selected {
                None => false,
                Some(idx) => idx == i,
            };
            let mut child = render_command_summary(cmd, child_width, 10, selected);
            let offset = child.size.height;

            ctr += offset;
            if ctr >= height {
                let spillover = ctr - height;
                let clip_pos = Position::new(0, spillover);
                let clip_size = Size::new(child_width, child.size.height - spillover);
                child = child.clip(clip_pos, clip_size).unwrap();
                child.position = Position::new(0, 0);
                ctr = height;
            }
            let child = child.offset(Position::new(0, height - ctr));
            children.push(child);
            if selected {
                let child_pos = Position::new(child_width, 0);
                let child_size = Size::new(size.width - child_width, size.height);
                let child = render_command_detail(cmd, child_size).offset(child_pos);
                children.push(child);
            }
            if ctr == height {
                break;
            }
            i += 1;
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
        let statew = shared(WeaverStateWidget::new(state.clone()));
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
        if text.len() > 0 {
            self.state
                .write()
                .unwrap()
                .run_command(text.clone())
                .unwrap();
        }
        self.statew.write().unwrap().selected = None;
    }

    fn log_msg(&mut self, msg: &str) {
        let lines: Vec<String> = msg.lines().map(|l| l.to_owned()).collect();
        self.log.write().unwrap().lines.extend(lines);
    }

    fn input(&mut self, key: Key) {
        match key {
            Key::Char('\n') => self.submit_input(),
            Key::Alt('\r') => self.input.write().unwrap().process_key(Key::Char('\n')),
            Key::Up => {
                let mut statew = self.statew.write().unwrap();
                statew.selected = match statew.selected.take() {
                    None => Some(0),
                    Some(i) => Some(i + 1),
                };
                if let Some(cmd) = statew.find_cmd_by_index(statew.selected.unwrap()) {
                    self.input.write().unwrap().set_line(&cmd);
                };
            }
            Key::Down => {
                let mut statew = self.statew.write().unwrap();
                statew.selected = match statew.selected.take() {
                    None => None,
                    Some(0) => None,
                    Some(i) => Some(i - 1),
                };
                match statew.selected {
                    Some(i) => if let Some(cmd) = statew.find_cmd_by_index(i) {
                        self.input.write().unwrap().set_line(&cmd);
                    },
                    None => self.input.write().unwrap().set_line(""),
                }
            }
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
            "stdout" => (None, Some(Box::new(color::Rgb(16, 32, 16)))),
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
            "selected.command" => (
                Some(Box::new(color::LightWhite)),
                Some(Box::new(color::Rgb(32, 32, 128))),
            ),
            "selected.stderr" => (
                Some(Box::new(color::LightWhite)),
                Some(Box::new(color::Rgb(64, 16, 16))),
            ),
            "selected.stdout" => (
                Some(Box::new(color::LightWhite)),
                Some(Box::new(color::Rgb(16, 64, 16))),
            ),
            "selected.command.running" => (
                Some(Box::new(color::LightYellow)),
                Some(Box::new(color::Rgb(16, 16, 32))),
            ),
            "selected.command.success" => (
                Some(Box::new(color::LightGreen)),
                Some(Box::new(color::Rgb(16, 16, 32))),
            ),
            "selected.command.failed" => (Some(Box::new(color::LightRed)), None),
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
