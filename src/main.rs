extern crate failure;
extern crate update_agent;

use std::env;
use std::process;

use failure::{bail, Error};
use update_agent::bits_server;
use update_agent::task;

fn main() {
    if let Err(err) = entry() {
        eprintln!("{}", err);
        for cause in err.iter_causes() {
            eprintln!("caused by {}", cause);
        }

        process::exit(1);
    }
}

const EXE_NAME: &'static str = "update_agent";

fn usage() -> String {
    format!(
        concat!(
            "Usage {0} <command> [args...]\n",
            "Commands:\n",
            "  install\n",
            "  uninstall\n",
            "  ondemand <command pipe>\n"
        ),
        EXE_NAME
    )
}

fn entry() -> Result<(), Error> {
    let args: Vec<_> = env::args_os().collect();

    if args.len() < 2 {
        eprintln!("{}", usage());
        bail!("not enough arguments");
    }

    let cmd = &*args[1].to_string_lossy();
    let cmd_args = &args[2..];

    match cmd {
        // scheduled task management
        "install" if cmd_args.is_empty() => task::install(),
        "uninstall" if cmd_args.is_empty() => task::uninstall(),

        // on-demand BITS intermediary task
        "ondemand" => bits_server::run(cmd_args),

        _ => {
            eprintln!("{}", usage());
            bail!("usage error");
        }
    }
}
