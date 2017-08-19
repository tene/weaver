extern crate liner;
extern crate bytes;
extern crate futures;
extern crate tokio_io;
extern crate tokio_proto;
extern crate tokio_service;
extern crate tokio_serde_cbor;

use liner::Context;
//use liner::KeyBindings;
use std::io::ErrorKind;

fn main() {
    let mut con = Context::new();
    loop {
        let line = con.read_line("weaver: ", &mut |_| {});
        match line {
            Ok(line) => {
                con.history.push(line.into()).unwrap();
            }
            Err(e) => {
                match e.kind() {
                    ErrorKind::Interrupted => {}
                    ErrorKind::UnexpectedEof => {
                        break;
                    }
                    _ => panic!("error: {:?}", e),
                }
            }
        }
    }
}

