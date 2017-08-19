extern crate bytes;
extern crate futures;
extern crate serde;
extern crate serde_cbor;
extern crate tokio_io;
extern crate tokio_proto;
extern crate tokio_service;
extern crate tokio_serde_cbor;
extern crate weaver;

use std::io;
//use tokio_serde_cbor::{Codec, Decoder, Encoder};

// XXX I've failed to work out how to use tokio_serde_cbor
// XXX Work around it with a wrapper that maps error types
//type MyCodec<Req, Rsp> = tokio_serde_cbor::Codec<Req, Rsp>;
type MyCodec<Req, Rsp> = weaver::Codec<Req, Rsp>;

use tokio_proto::streaming::multiplex::ServerProto;

pub struct LineProto;

use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::Framed;

impl<T: AsyncRead + AsyncWrite + 'static> ServerProto<T> for LineProto {
    /// For this protocol style, `Request` matches the `Item` type of the codec's `Encoder`
    type Request = String;

    /// For this protocol style, `Response` matches the `Item` type of the codec's `Decoder`
    type Response = String;

    /// A bit of boilerplate to hook in the codec:
    type Transport = Framed<T, MyCodec<Self::Request, Self::Response>>;
    type BindTransport = Result<Self::Transport, io::Error>;
    fn bind_transport(&self, io: T) -> Self::BindTransport {
        Ok(io.framed(MyCodec::new()))
    }
}

use tokio_service::Service;

pub struct Echo;

use futures::{future, Future, BoxFuture};

impl Service for Echo {
    type Request = String;
    type Response = String;

    type Error = io::Error;

    type Future = BoxFuture<Self::Response, Self::Error>;

    fn call(&self, req: Self::Request) -> Self::Future {
        future::ok(req).boxed()
    }
}

use tokio_proto::TcpServer;

fn main() {
    let addr = "0.0.0.0:12345".parse().unwrap();
    let server = TcpServer::new(LineProto, addr);
    server.serve(|| Ok(Echo));
}

