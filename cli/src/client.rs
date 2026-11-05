//! One-shot command-line client of a running alifib server.

use std::net::TcpStream;
use std::process::ExitCode;

use alifib::interactive::kernel::{Reply, Status};
use alifib::interactive::rpc::{eval_call, eval_replies};

use crate::display::Display;
use crate::http;

/// Run one line remotely, printing its replies as the in-process REPL does.
/// Transport and protocol failures go to stderr; a reply that failed fails
/// the exit status.
pub fn run(address: &str, line: &str) -> ExitCode {
    match call(address, line) {
        Ok(replies) => {
            let display = Display::stdout();
            replies.iter().for_each(|reply| display.reply(reply));
            if replies.iter().any(|reply| reply.status == Status::Error) {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

/// One unguarded, pageless `eval` call. There is no read deadline: the kernel
/// may compute for as long as the command needs.
fn call(address: &str, line: &str) -> Result<Vec<Reply>, String> {
    let stream = TcpStream::connect(address).map_err(|error| {
        format!(
            "no alifib server at {address} ({error}) — start one with 'alifib serve' or 'alifib web'"
        )
    })?;
    http::write_request(
        &stream,
        "POST",
        "/api",
        &[("Host", address), ("Content-Type", "application/json")],
        &eval_call(line),
    )
    .map_err(|error| format!("could not send command: {error}"))?;
    let response = http::read_response(&stream)
        .map_err(|error| format!("could not read server reply: {error}"))?;
    if response.status != 200 {
        return Err(format!("{}\n{}", response.line, response.body));
    }
    eval_replies(&response.body).map_err(|message| format!("server error: {message}"))
}
