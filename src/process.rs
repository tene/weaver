extern crate mio;

use self::mio::event::Evented;
use self::mio::unix::{EventedFd, UnixReady};
use self::mio::{PollOpt, Ready, Token};

use super::futures::future::FlattenStream;
use super::futures::{Async, Future, Poll, Stream};
use super::libc;
use super::tokio::reactor::PollEvented2 as PollEvented;
use super::tokio_io::IoFuture;
use super::tokio_signal::unix::Signal;

use std::io;
use std::os::unix::prelude::*;
use std::process::{self, ExitStatus};

pub struct Child {
    inner: process::Child,
    reaped: bool,
    sigchld: FlattenStream<IoFuture<Signal>>,
}

impl Child {
    pub fn new(inner: process::Child) -> Child {
        Child {
            inner: inner,
            reaped: false,
            sigchld: Signal::new(libc::SIGCHLD).flatten_stream(),
        }
    }

    pub fn id(&self) -> u32 {
        self.inner.id()
    }

    pub fn kill(&mut self) -> io::Result<()> {
        if !self.reaped {
            // NB: SIGKILL cannnot be caught, so the process will definitely exit immediately.
            // We're not waiting for the process itself but for the kernel to execute the kill.
            self.inner.kill()?;
            let _ = self.try_wait(true);
        }

        Ok(())
    }

    pub fn poll_exit(&mut self) -> Poll<ExitStatus, io::Error> {
        loop {
            // Ensure that once we've successfully waited we won't try to
            // `kill` above.
            if let Some(e) = try!(self.try_wait(false)) {
                return Ok(e.into());
            }

            // If the child hasn't exited yet, then it's our responsibility to
            // ensure the current task gets notified when it might be able to
            // make progress.
            //
            // As described in `spawn` above, we just indicate that we can
            // next make progress once a SIGCHLD is received.
            if try!(self.sigchld.poll()).is_not_ready() {
                return Ok(Async::NotReady);
            }
        }
    }

    fn try_wait(&mut self, block_on_wait: bool) -> io::Result<Option<ExitStatus>> {
        assert!(!self.reaped);
        let exit = try!(try_wait_process(self.id() as libc::pid_t, block_on_wait));

        if let Some(_) = exit {
            self.reaped = true;
        }

        Ok(exit)
    }
}

fn try_wait_process(id: libc::pid_t, block_on_wait: bool) -> io::Result<Option<ExitStatus>> {
    let wait_flags = if block_on_wait { 0 } else { libc::WNOHANG };
    let mut status = 0;

    loop {
        match unsafe { libc::waitpid(id, &mut status, wait_flags) } {
            0 => return Ok(None),
            n if n < 0 => {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }
            n => {
                assert_eq!(n, id);
                return Ok(Some(ExitStatus::from_raw(status)));
            }
        }
    }
}

impl Future for Child {
    type Item = ExitStatus;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<ExitStatus, io::Error> {
        self.poll_exit()
    }
}

#[derive(Debug)]
pub struct Fd<T>(T);

impl<T: io::Read> io::Read for Fd<T> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.0.read(bytes)
    }
}

impl<T: io::Write> io::Write for Fd<T> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

pub type ChildStdin = PollEvented<Fd<process::ChildStdin>>;
pub type ChildStdout = PollEvented<Fd<process::ChildStdout>>;
pub type ChildStderr = PollEvented<Fd<process::ChildStderr>>;

impl<T> Evented for Fd<T>
where
    T: AsRawFd,
{
    fn register(
        &self,
        poll: &mio::Poll,
        token: Token,
        interest: Ready,
        opts: PollOpt,
    ) -> io::Result<()> {
        EventedFd(&self.0.as_raw_fd()).register(poll, token, interest | UnixReady::hup(), opts)
    }

    fn reregister(
        &self,
        poll: &mio::Poll,
        token: Token,
        interest: Ready,
        opts: PollOpt,
    ) -> io::Result<()> {
        EventedFd(&self.0.as_raw_fd()).reregister(poll, token, interest | UnixReady::hup(), opts)
    }

    fn deregister(&self, poll: &mio::Poll) -> io::Result<()> {
        EventedFd(&self.0.as_raw_fd()).deregister(poll)
    }
}

pub fn stdio<T>(io: T) -> io::Result<PollEvented<Fd<T>>>
where
    T: AsRawFd,
{
    // Set the fd to nonblocking before we pass it to the event loop
    unsafe {
        let fd = io.as_raw_fd();
        let r = libc::fcntl(fd, libc::F_GETFL);
        if r == -1 {
            return Err(io::Error::last_os_error());
        }
        let r = libc::fcntl(fd, libc::F_SETFL, r | libc::O_NONBLOCK);
        if r == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    let io = PollEvented::new(Fd(io));
    Ok(io)
}
