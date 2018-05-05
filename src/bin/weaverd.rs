extern crate tokio;
extern crate tokio_io;
extern crate tokio_uds;
extern crate tokio_serde_msgpack;
extern crate rmp_serde;
extern crate futures;
extern crate weaver;

use futures::sync::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};

use tokio::prelude::{Sink, Stream, Future};
use tokio_uds::{UnixListener, UnixStream};
use tokio_serde_msgpack::{from_io, MsgPackReader, MsgPackWriter};


use weaver::{ClientMessage, ServerMessage};

pub struct ClientConn {
    pub send_buf: UnboundedSender<ServerMessage>,
}

impl ClientConn {
    pub fn new(socket: UnixStream) -> Self {
        let (socket_rx, socket_tx): (MsgPackReader<UnixStream, ClientMessage>, MsgPackWriter<UnixStream, ServerMessage>) = from_io(socket);
        let socket_tx = socket_tx.sink_map_err(|e| println!("Send Err: {:#?}", e));
        let (chan_send, chan_recv): (UnboundedSender<ServerMessage>, UnboundedReceiver<ServerMessage>) = unbounded();
        let forward_to_client = socket_tx
            .send_all(chan_recv)
            .then(|_| Ok(()));
        tokio::spawn(forward_to_client);

        let asdf = chan_send.clone();
        let handle_messages = socket_rx
            .for_each(move |msg| {
                let response = ServerMessage {
                    id: msg.id,
                    name: format!("server {}", msg.name),
                };
                let _ = asdf.unbounded_send(response);
                Ok(())
            })
            .map_err(|e| println!("Recv Err: {:#?}", e));
        tokio::spawn(handle_messages);

        ClientConn {
            send_buf: chan_send,
        }
    }
    pub fn handle_msg(&mut self, _msg: ClientMessage) {
    }
}

fn main() {
    let mut socketpath = std::env::home_dir().unwrap();
    socketpath.push(".weaver.socket");
    let _ = std::fs::remove_file(&socketpath);

    let listener = UnixListener::bind(socketpath).unwrap();
    let server = listener.incoming()
    .map_err(|e| println!("error = {:?}", e))
    .for_each(move |(socket, _addr)| {
        let _client = ClientConn::new(socket);
        Ok(())
    });

    tokio::run(server);
    println!("Hello, world!");
}
