use std::error::Error;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::os::unix::io::{FromRawFd, IntoRawFd, RawFd};

use nix::fcntl::OFlag;
use nix::pty;
use nix::sys::termios;

use mio::unix::SourceFd;
use mio::{Events, Interest, Poll, Token};
use nix::pty::PtyMaster;

const STDIN: Token = Token(0);
const PTY_MASTER: Token = Token(1);

/// A PTY master / slave pair.
struct PtyPair {
    master: pty::PtyMaster,
    slave_name: String,
}

/// Configures the given term to be in 'raw' mode.
fn term_set_raw(fd: RawFd, termios: &mut termios::Termios) -> Result<(), nix::Error> {
    termios::cfmakeraw(termios);
    termios::tcsetattr(fd, termios::SetArg::TCSANOW, termios)
}

/// Proxies between stdin of this process to the master terminal device.
fn proxy_term(stdin: RawFd, pty_master: PtyMaster) -> Result<(), Box<dyn Error>> {
    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(128);
    let mut buf: [u8; 2048] = [0; 2048];

    // Use IntoRawFd here because we want to transfer ownership of the
    // file descriptor to this function. This prevents a "double drop"
    // when main returns.
    let pty_master_fd = pty_master.into_raw_fd();
    let fpty_master: &mut File = unsafe { &mut File::from_raw_fd(pty_master_fd) };

    // We have to use a raw handle to the stdout stream due to the
    // lock that is put on it if we get a reference via io::stdout().
    // If we get a locked reference you cannot see what you're typing
    // as you're typing it.
    let fstdout: &mut File = unsafe { &mut File::from_raw_fd(1) };

    // Register stdin, wait for it to be readable.
    poll.registry()
        .register(&mut SourceFd(&stdin), STDIN, Interest::READABLE)?;

    // Register PTY master, wait for it to be readable.
    poll.registry().register(
        &mut SourceFd(&pty_master_fd),
        PTY_MASTER,
        Interest::READABLE,
    )?;

    loop {
        // Poll for events, blocking until we get an event.
        poll.poll(&mut events, None)?;

        // Process each event.
        for event in events.iter() {
            if event.is_read_closed() {
                return Ok(());
            }
            match event.token() {
                STDIN => {
                    let n = io::stdin().read(&mut buf)?;
                    fpty_master.write_all(&mut buf[0..n])?;
                }
                PTY_MASTER => {
                    let n = fpty_master.read(&mut buf)?;
                    fstdout.write_all(&mut buf[0..n])?;
                }
                // We don't expect any events with tokens other than those we provided.
                _ => unreachable!(),
            }
        }
    }
}

fn new_pty() -> Result<PtyPair, Box<dyn Error>> {
    // Open a new PTY master.
    let master = pty::posix_openpt(OFlag::O_RDWR)?;

    // Allow a slave to be generated for it.
    pty::grantpt(&master)?;
    pty::unlockpt(&master)?;

    // Get the name of the slave.
    let slave_name = unsafe { pty::ptsname(&master) }?;

    Ok(PtyPair { master, slave_name })
}

fn main() -> Result<(), Box<dyn Error>> {
    let stdin: RawFd = 0;

    // Get the termios config for the terminal connected to this process.
    let mut termios = termios::tcgetattr(stdin)?;

    // Set the current terminal to 'raw' mode.
    term_set_raw(stdin, &mut termios)?;

    // Open a new pty master device.
    let pty_pair = new_pty()?;

    println!("Opened new PTY device: {}", pty_pair.slave_name);

    // Proxy between our stdin device and the PTY master device.
    proxy_term(stdin, pty_pair.master)
}
