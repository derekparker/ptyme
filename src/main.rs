use std::error::Error;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};

use nix::fcntl::OFlag;
use nix::sys::termios;
use nix::{pty, unistd};

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

/// Writes the buffer `rdr` to the writer `f`.
/// Always calls `f.flush()`.
fn write_buffer_to(mut rdr: impl BufRead, mut f: impl Write) -> Result<(), Box<dyn Error>> {
    let buf = rdr.fill_buf()?;
    f.write_all(buf)?;
    f.flush()?;
    let len = buf.len();
    rdr.consume(len);

    Ok(())
}

/// Proxies between stdin of this process to the master terminal device.
fn proxy_term(stdin: RawFd, pty_master: PtyMaster) -> Result<(), Box<dyn Error>> {
    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(128);
    let pty_master_fd = unistd::dup(pty_master.as_raw_fd())?;
    let fpty_master: File = unsafe { File::from_raw_fd(pty_master_fd) };
    let mut fpty_master = BufReader::new(fpty_master);

    // Register stdin, wait for it to be readable.
    poll.registry()
        .register(&mut SourceFd(&stdin), STDIN, Interest::READABLE)?;

    // Register PTY master, wait for it to be readable.
    poll.registry().register(
        &mut SourceFd(&pty_master_fd),
        PTY_MASTER,
        Interest::READABLE,
    )?;

    // Grab handle and lock stdin to prevent excess locking during
    // our loop below.
    let stdin = io::stdin();
    let mut stdin_hdl = stdin.lock();
    let stdout = io::stdout();
    let mut stdout_hdl = stdout.lock();

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
                    write_buffer_to(&mut stdin_hdl, fpty_master.get_mut())?;
                }
                PTY_MASTER => {
                    write_buffer_to(&mut fpty_master, &mut stdout_hdl)?;
                }
                // We don't expect any events with tokens other than those we provided.
                _ => unreachable!(),
            }
        }
    }
}

/// Opens and returns a new PTY pair.
/// The pair contains the PTY master FD and the slave path.
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
    let saved = termios::tcgetattr(stdin)?;
    let mut termios = saved.clone();

    // Open a new pty master device.
    let pty_pair = new_pty()?;

    println!("Opened new PTY device: {}", pty_pair.slave_name);

    // Set the current terminal to 'raw' mode.
    term_set_raw(stdin, &mut termios)?;

    // Proxy between our stdin device and the PTY master device.
    proxy_term(stdin, pty_pair.master)?;

    // Restore the terminal to its original settings.
    termios::tcsetattr(stdin, termios::SetArg::TCSANOW, &saved)?;

    Ok(())
}
