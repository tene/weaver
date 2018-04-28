extern crate text_ui;
use text_ui::app::App;
use text_ui::backend::Backend;
use text_ui::widget::{shared, Linear, Shared, Text, TextInput};
use text_ui::{Event, Input, Key};

struct WeaverTui {
    log: Shared<Text>,
    input: Shared<TextInput>,
    vbox: Shared<Linear>,
}

impl WeaverTui {
    fn new() -> WeaverTui {
        let log = shared(Text::new(vec![]));
        let input = shared(TextInput::new(""));
        let mut mainbox = Linear::vbox();
        mainbox.push(&log);
        mainbox.push(&input);
        let vbox = shared(mainbox);
        WeaverTui {
            log: log,
            input: input,
            vbox: vbox,
        }
    }

    fn submit_input(&mut self) {
        self.log
            .write()
            .unwrap()
            .push(self.input.write().unwrap().submit());
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

#[derive(Debug, PartialEq, Clone)]
enum WeaverEvent {
    _Ping,
}

impl App for WeaverTui {
    type UI = Shared<Linear>;
    type MyEvent = WeaverEvent;
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
            Event::AppEvent(_) => {
                Ok(())
            }
        }
    }
}

fn main() {
    let mut be = Backend::new();
    let mut app = WeaverTui::new();
    app.log_msg("Esc to exit");
    be.run_app(&mut app);
}
